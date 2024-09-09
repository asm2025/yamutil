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
use std::sync::Arc;
use structopt::StructOpt;

use crate::{common::*, error::*};

const BASE_URL: &str = "https://www.yammer.com/api/v1/";

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
    let mut has_more = true;
    let mut last_message_id = None;

    while has_more {
        let (messages, more) =
            get_messages(client, token, user_id.clone(), last_message_id).await?;
        has_more = more;

        if messages.is_empty() {
            break;
        }

        for message in messages {
            println!(
                "ID: {}, Sender ID: {}, Created At: {}, Body: {}",
                message["id"], message["sender_id"], message["created_at"], message["body"]["rich"]
            );
            last_message_id = message["id"].as_u64();
        }
    }

    return Ok(());
}

async fn delete(client: &Client, token: &str, user_id: Option<u64>) -> Result<()> {
    return Ok(());
}

async fn get_messages(
    client: &Client,
    token: &str,
    user_id: Option<u64>,
    last_message_id: Option<u64>,
) -> Result<(Vec<Value>, bool)> {
    let p_message = if let Some(lmid) = last_message_id {
        format!("&older_than={}", lmid)
    } else {
        String::new()
    };
    let url = format!("{}messages/sent.json?threaded=true{}", BASE_URL, p_message);
    let response = client
        .get(&url)
        .header("authorization", format!("Bearer {}", &token))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Box::new(response.error_for_status().unwrap_err()));
    }

    let feed = response.json::<Value>().await?;
    let mut messages = feed["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["sender_type"] == "user")
        .cloned()
        .collect::<Vec<_>>();

    if let Some(id) = user_id {
        messages.retain(|m| m["sender_id"].as_u64().unwrap_or(0) == id);
    }

    let older_available = feed["meta"]["older_available"].as_bool().unwrap_or(false);
    return Ok((messages, older_available));
}
