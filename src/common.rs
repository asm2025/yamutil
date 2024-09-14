use clap::{command, ArgGroup, Parser, Subcommand};
use lazy_static::lazy_static;
use reqwest_cookie_store::CookieStoreRwLock;
use rustmix::{
    io::directory,
    random,
    web::reqwest::{
        build_client_with_user_agent,
        header::{self, HeaderMap, HeaderValue},
        redirect, Client,
    },
    AppInfo, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{to_string_pretty, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(debug_assertions)]
pub const TIMEOUT: u64 = 30;
#[cfg(not(debug_assertions))]
pub const TIMEOUT: u64 = 5;

pub const BASE_URL: &str = "https://www.yammer.com/api/v1/";
pub const RATE_LIMIT_TIMEOUT_MAX: u64 = 300;

const ARGSGRP_GROUP_OR_THREAD: &str = "EitherGroupOrThread";

lazy_static! {
    pub static ref APP_INFO: Arc<AppInfo<'static>> = Arc::new(AppInfo::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
        Some(env!("CARGO_PKG_DESCRIPTION")),
        Some(env!("CARGO_PKG_LICENSE")),
    ));
    pub static ref CURDIR: PathBuf = directory::current();
    pub static ref LOGDIR: PathBuf = CURDIR.join("_logs");
    static ref WAIT_TIME: Duration = Duration::from_secs(2);
}

#[derive(Debug, Parser)]
#[command(
    name = env!("CARGO_PKG_NAME"),
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS"),
    about = env!("CARGO_PKG_DESCRIPTION")
)]
pub struct Args {
    /// The action to take on Yammer user's posts.
    #[command(subcommand)]
    pub action: Option<YammerAction>,
    /// Enable debug mode. The build must be a debug build.
    #[arg(short, long)]
    pub debug: bool,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum YammerAction {
    /// List messages.
    #[command(group(ArgGroup::new(ARGSGRP_GROUP_OR_THREAD).args(&["group_id", "thread_id"])))]
    List {
        /// The Yammer application token.
        #[arg(short, long, required = true)]
        token: String,
        /// The message group id. If no group id is provided, all messages will be listed.
        #[arg(short, long, group = ARGSGRP_GROUP_OR_THREAD)]
        group_id: Option<u64>,
        /// The message thread id. If no thread id is provided, all messages will be listed.
        #[arg(short = 'i', long, group = ARGSGRP_GROUP_OR_THREAD)]
        thread_id: Option<u64>,
        /// The user email to filter posts.
        #[arg(short, long)]
        email: Option<String>,
        /// This will list the full messages' threads.
        #[arg(short, long)]
        all: bool,
    },
    /// Delete messages.
    #[command(group(ArgGroup::new(ARGSGRP_GROUP_OR_THREAD).args(&["group_id", "thread_id"])))]
    Delete {
        /// The Yammer application token.
        #[arg(short, long, required = true)]
        token: String,
        /// The message group id. If no group id is provided, all messages will be listed.
        #[arg(short, long, group = ARGSGRP_GROUP_OR_THREAD)]
        group_id: Option<u64>,
        /// The message thread id. If no thread id is provided, all messages will be deleted.
        #[arg(short = 'i', long, group = ARGSGRP_GROUP_OR_THREAD)]
        thread_id: Option<u64>,
        /// The user email to filter posts.
        #[arg(short, long)]
        email: Option<String>,
        /// Message IDs to exclude from deletion.
        #[arg(short = 'x', long)]
        exclude: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity: usize,
    tokens: usize,
    rate_in_seconds: u64,
    updated: Instant,
}

impl TokenBucket {
    pub fn new(capacity: usize, rate: u64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            rate_in_seconds: rate,
            updated: Instant::now(),
        }
    }

    pub fn take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.updated).as_secs();
        self.updated = now;
        self.tokens = (self.tokens as u64 + elapsed * self.rate_in_seconds)
            .min(self.capacity as u64) as usize;

        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YammerUser {
    pub id: u64,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YammerGroup {
    pub id: u64,
    pub name: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YammerMessage {
    pub id: u64,
    pub replied_to_id: Option<u64>,
    pub sender_id: u64,
    pub network_id: u64,
    pub group_id: u64,
    pub group_name: String,
    pub thread_id: u64,
    pub privacy: String,
    pub created_at: String,
    pub body: String,
    pub liked_by: u64,
    pub replies: Option<Vec<YammerMessage>>,
}

pub fn to_yammer_message(message: &Value, groups: &HashMap<u64, YammerGroup>) -> YammerMessage {
    let group_id = message["group_id"].as_u64().unwrap_or(0);
    let group_name_def = group_id.to_string();
    let group_name = groups
        .get(&group_id)
        .map(|e| &e.display_name)
        .unwrap_or(&group_name_def);
    YammerMessage {
        id: message["id"].as_u64().unwrap_or(0),
        replied_to_id: message["replied_to_id"].as_u64(),
        sender_id: message["sender_id"].as_u64().unwrap_or(0),
        network_id: message["network_id"].as_u64().unwrap_or(0),
        group_id: group_id,
        group_name: group_name.to_owned(),
        thread_id: message["thread_id"].as_u64().unwrap_or(0),
        privacy: message["privacy"].as_str().unwrap().to_owned(),
        created_at: message["created_at"].as_str().unwrap().to_owned(),
        body: message["body"]["rich"].as_str().unwrap().to_owned(),
        liked_by: message["liked_by"]["count"].as_u64().unwrap_or(0),
        replies: None,
    }
}

pub fn build_compatible_client(cookies: &Arc<CookieStoreRwLock>) -> Result<Client> {
    cookies.write().unwrap().clear();

    let user_agent = random_ua();
    let mut headers = HeaderMap::new();
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_str(&user_agent).unwrap(),
    );
    headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate"),
    );
    headers.insert(
        header::UPGRADE_INSECURE_REQUESTS,
        HeaderValue::from_static("1"),
    );
    let client = build_client_with_user_agent(user_agent.to_owned())
        .default_headers(headers)
        .cookie_provider(cookies.clone())
        .redirect(redirect::Policy::limited(u8::MAX as usize))
        .timeout(Duration::from_secs(TIMEOUT))
        .build()?;
    Ok(client)
}

fn random_ua() -> String {
    match random::numeric(0..2) {
        0 => random::internet::user_agent().safari().to_string(),
        1 => random::internet::user_agent().firefox().to_string(),
        _ => random::internet::user_agent().chrome().to_string(),
    }
}

pub fn print_json(message: &Value) {
    let json = to_string_pretty(&message).unwrap();
    println!("{}", json);
}

pub fn print_message(message: &YammerMessage) {
    let json = to_string_pretty(&message).unwrap();
    println!("{}", json);
}

pub mod output {
    use super::*;

    pub fn print_header(appinfo: &AppInfo) {
        println!(
            r#"
★····························································★
      _____   ° _________   _.★    ·               _____.·★
    ·/  _  \ ★ /   _____/_/  |_  _______ ¤__ __★  /     \  ·
  ★ /  /_\  \  \_____  \ \   ___\\_  __ \|  |  \ /  \ /  \·
 · /    |    \ /        \ |  | ·  |  | \/|  |  //    Y    \ ★
   \____|____//_________/ |__|  · |__|★  |____/ \____|____/·
  ★·.°      ¤        °·★                ¤··•      ★·
★····························································★
·★·.·´¯`·.·★ {} v{} ★·.·´¯`·.·★
·.•°¤*(¯`★´¯)*¤° {} °¤*)¯´★`¯(*¤°•.
·★ {} ★·
"#,
            appinfo.name, appinfo.version, appinfo.authors, appinfo.description
        );
    }
}
