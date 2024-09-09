mod common;
mod error;

use chrono::Local;
use dotenv::dotenv;
use log::{error, info, warn};
use reqwest_cookie_store::{CookieStore, CookieStoreRwLock};
use rustmix::{
    error::*,
    log4rs::{
        self,
        config::{runtime::Config, Logger, Root},
    },
    web::reqwest::Client,
    *,
};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, time::Duration};
use structopt::StructOpt;
use tokio::time::sleep;

use crate::{common::*, error::*};

const BASE_URL: &str = "https://www.yammer.com/api/v1/";
const RATE_LIMIT_TIMEOUT_MAX: u64 = 300;

#[derive(Debug, StructOpt)]
#[structopt(
    name = env!("CARGO_PKG_NAME"),
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS"),
    about = env!("CARGO_PKG_DESCRIPTION")
)]
struct Args {
    #[structopt(short, long, required = true, help = "The Yammer application token.")]
    token: String,
    #[structopt(short, long, help = "The user email to filter posts.")]
    email: Option<String>,
    #[structopt(short, long, default_value = YammerAction::List.into(), help = "The action to take on Yammer user's posts.")]
    action: YammerAction,
    #[structopt(short, long, help = "Enable debug mode. The build is a debug build.")]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Called first to set debug flag. It affects the log level
    let args = Args::from_args();
    set_debug(args.debug);
    let gaurd = log4rs::from_config(configure_log()?)?;

    output::print_header(&APP_INFO);
    info!("{} v{} started", APP_INFO.name, APP_INFO.version);

    let cookies = Arc::new(CookieStoreRwLock::new(CookieStore::default()));
    let client = match build_compatible_client(&cookies) {
        Ok(it) => Arc::new(it),
        Err(e) => {
            panic!("Error building client: {}", e.get_message());
        }
    };
    let user_id = if let Some(email) = &args.email {
        match get_user_id(&client, &args.token, &email).await {
            Ok(it) => it,
            Err(e) => {
                error!("{}", e.get_message());
                return Ok(());
            }
        }
    } else {
        None
    };

    match args.action {
        YammerAction::List => match list(&client, &args.token, user_id).await {
            Ok(_) => {}
            Err(e) => {
                error!("{}", e.get_message());
            }
        },
        YammerAction::Delete => match delete(&client, &args.token, user_id).await {
            Ok(_) => {}
            Err(e) => {
                error!("{}", e.get_message());
            }
        },
    }

    info!("{} v{} finished", APP_INFO.name, APP_INFO.version);
    drop(gaurd);
    Ok(())
}

fn configure_log() -> Result<Config> {
    let log_level = if is_debug() {
        LogLevel::Debug
    } else {
        LogLevel::Info
    };
    let logger = log4rs::configure(
        LOGDIR.join(Local::now().format("fb-%Y%m%d.log").to_string()),
        log_level,
        None,
    )?
    .logger(Logger::builder().build("hyper_util", log::LevelFilter::Warn))
    .logger(Logger::builder().build("tokenizers", log::LevelFilter::Error));
    let config = logger.build(
        Root::builder()
            .appender("console")
            .appender("file")
            .build(log_level.into()),
    )?;
    Ok(config)
}

async fn get_user_id(client: &Client, token: &str, user_email: &str) -> Result<Option<u64>> {
    if user_email.is_empty() {
        return Ok(None);
    }

    info!("Trying to get user id from user email");
    let url = format!(
        "{}users/by_email.json?email={}",
        BASE_URL,
        urlencoding::encode(&user_email)
    );
    let response = client
        .get(&url)
        .header("authorization", format!("Bearer {}", &token))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Box::new(response.error_for_status().unwrap_err()));
    }

    let json = response.json::<serde_json::Value>().await?;
    let id = json
        .as_array()
        .and_then(|items| items.iter().find(|u| u["type"] == "user"))
        .and_then(|user| user["id"].as_u64());

    if let Some(id) = id {
        info!("User id for email '{}' is found: {}", user_email, id);
        return Ok(Some(id));
    }

    warn!("User id for email '{}' is not found", user_email);
    return Ok(None);
}

async fn list(client: &Client, token: &str, user_id: Option<u64>) -> Result<()> {
    let mut messages = Vec::new();
    let mut thread_messages = Vec::new();
    let mut has_more = true;
    let mut last_message_id = None;
    let mut message_count = 0u64;

    info!("Fetching messages");

    while has_more {
        has_more = get_messages(
            &mut messages,
            client,
            token,
            user_id.clone(),
            last_message_id,
        )
        .await?;
        message_count += messages.len() as u64;

        while !messages.is_empty() {
            let message = messages.remove(0);
            last_message_id = message["id"].as_u64();
            let thread_id = message["thread_id"].as_u64().unwrap();
            info!("Fetching messages for thread {}", thread_id);

            if !get_messages_in_thread(&mut thread_messages, client, token, thread_id).await? {
                continue;
            }

            println!("Messages for thread {}", thread_id);

            while !thread_messages.is_empty() {
                let thread_message = thread_messages.remove(0);
                print_message(&thread_message);
            }
        }
    }

    info!("Listed {} messages", message_count);
    return Ok(());
}

async fn delete(client: &Client, token: &str, user_id: Option<u64>) -> Result<()> {
    let mut has_more = true;
    let mut messages: HashMap<u64, Vec<Value>> = HashMap::new();
    let mut q_messages = Vec::new();
    let mut qt_messages = Vec::new();
    let mut last_message_id = None;
    let mut deleted_messages = 0u64;

    info!("Fetching messages");

    while has_more {
        has_more = get_messages(
            &mut q_messages,
            client,
            token,
            user_id.clone(),
            last_message_id,
        )
        .await?;

        if q_messages.is_empty() {
            continue;
        }

        // intialize the message queue for each thread id
        for message in &q_messages {
            let thread_id = message["thread_id"].as_u64().unwrap();
            last_message_id = message["id"].as_u64();
            messages.insert(thread_id, Vec::new());
        }

        // add top level messages to the queue
        while let Some(message) = q_messages.pop() {
            let thread_id = message["thread_id"].as_u64().unwrap();

            if has_likes(&message) {
                messages.remove(&thread_id);
                continue;
            }

            // traverse the message tree and add all messages to the queue.
            info!("Traversing the message tree for thread {}", thread_id);

            if !get_messages_in_thread(&mut qt_messages, client, token, thread_id).await? {
                continue;
            }

            while !qt_messages.is_empty() {
                let thread_message = qt_messages.remove(0);

                if has_likes(&thread_message) || thread_message["sender_id"].as_u64() != user_id {
                    messages.remove(&thread_id);
                    continue;
                }

                let queue = match messages.get_mut(&thread_id) {
                    Some(queue) => queue,
                    None => continue,
                };
                queue.push(thread_message.clone());
            }
        }

        // delete messages in the queue
        for (_, queue) in messages.iter() {
            for message in queue.iter() {
                let id = message["id"].as_u64().unwrap();
                info!("Deleting message '{}'", id);
                let url = format!("{}messages/{}.json", BASE_URL, id);
                // let response = client
                //     .delete(&url)
                //     .header("authorization", format!("Bearer {}", &token))
                //     .send()
                //     .await?;

                // if !response.status().is_success() {
                //     error!(
                //         "Error deleting message '{}': {}",
                //         id,
                //         response.text().await?
                //     );
                //     continue;
                // }
                println!("{}", &url);
                deleted_messages += 1;
                info!("Deleted message '{}'", id);
            }
        }
    }

    info!("Deleted {} messages", deleted_messages);
    return Ok(());
}

async fn get_messages(
    collection: &mut Vec<Value>,
    client: &Client,
    token: &str,
    user_id: Option<u64>,
    last_message_id: Option<u64>,
) -> Result<bool> {
    let p_message = if let Some(lmid) = last_message_id {
        format!("&older_than={}", lmid)
    } else {
        String::new()
    };
    let url = format!("{}messages/sent.json?threaded=true{}", BASE_URL, p_message);
    let mut rate_limit_timeout = 5u64;
    let response = loop {
        match client
            .get(&url)
            .header("authorization", format!("Bearer {}", &token))
            .send()
            .await
        {
            Ok(it) => {
                if it.status() == 429 {
                    if rate_limit_timeout > RATE_LIMIT_TIMEOUT_MAX {
                        return Err(Box::new(RateLimitTimeoutExceededError));
                    }
                    warn!(
                        "Rate limit exceeded. Waiting for {} seconds",
                        rate_limit_timeout
                    );
                    sleep(Duration::from_secs(rate_limit_timeout)).await;
                    rate_limit_timeout = rate_limit_timeout + 5;
                    continue;
                } else {
                    break it;
                }
            }
            Err(e) => return Err(Box::new(e)),
        }
    };

    if !response.status().is_success() {
        return Err(Box::new(response.error_for_status().unwrap_err()));
    }

    let feed = response.json::<Value>().await?;
    let messages = feed["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["sender_type"] == "user" && (user_id.is_none() || e["sender_id"].as_u64() == user_id)
        })
        .cloned();
    collection.extend(messages);
    let older_available = feed["meta"]["older_available"].as_bool().unwrap_or(false);
    return Ok(older_available);
}

async fn get_messages_in_thread(
    collection: &mut Vec<Value>,
    client: &Client,
    token: &str,
    thread_id: u64,
) -> Result<bool> {
    let url = format!("{}messages/in_thread/{}.json", BASE_URL, &thread_id);
    let mut rate_limit_timeout = 5u64;
    let response = loop {
        match client
            .get(&url)
            .header("authorization", format!("Bearer {}", &token))
            .send()
            .await
        {
            Ok(it) => {
                if it.status() == 429 {
                    if rate_limit_timeout > RATE_LIMIT_TIMEOUT_MAX {
                        return Err(Box::new(RateLimitTimeoutExceededError));
                    }
                    warn!(
                        "Rate limit exceeded. Waiting for {} seconds",
                        rate_limit_timeout
                    );
                    sleep(Duration::from_secs(rate_limit_timeout)).await;
                    rate_limit_timeout = rate_limit_timeout + 5;
                    continue;
                } else {
                    break it;
                }
            }
            Err(e) => return Err(Box::new(e)),
        }
    };

    if !response.status().is_success() {
        return Err(Box::new(response.error_for_status().unwrap_err()));
    }

    let count = collection.len();
    let feed = response.json::<Value>().await?;
    let messages = feed["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["sender_type"] == "user")
        .cloned();
    collection.extend(messages);
    return Ok(collection.len() > count);
}

fn print_message(message: &Value) {
    println!(
        "ID: {}, Sender ID: {}, Created At: {}, Network Id: {}, Group Id: {}, Thread Id: {}, Privacy: {}, Body: {}, Liked By: {}",
        message["id"], message["sender_id"], message["created_at"], message["network_id"], message["group_id"], message["thread_id"], message["privacy"], message["body"]["rich"], message["liked_by"]["count"]
    );
}

fn has_likes(message: &Value) -> bool {
    let liked_by = message["liked_by"]["count"].as_u64().unwrap_or(0);
    return liked_by > 0;
}
