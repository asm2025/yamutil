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
use std::str::FromStr;
use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

use crate::error::*;

lazy_static! {
    pub static ref APP_INFO: AppInfo<'static> = AppInfo::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
        Some(env!("CARGO_PKG_DESCRIPTION")),
        Some(env!("CARGO_PKG_LICENSE")),
    );
    pub static ref CURDIR: PathBuf = directory::current();
    pub static ref LOGDIR: PathBuf = CURDIR.join("_logs");
    static ref WAIT_TIME: Duration = Duration::from_secs(2);
}

#[cfg(debug_assertions)]
pub const TIMEOUT: u64 = 30;
#[cfg(not(debug_assertions))]
pub const TIMEOUT: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum YammerAction {
    List,
    Delete,
}

impl fmt::Display for YammerAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            YammerAction::List => write!(f, "List"),
            YammerAction::Delete => write!(f, "Delete"),
        }
    }
}

impl FromStr for YammerAction {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "List" => Ok(YammerAction::List),
            "Delete" => Ok(YammerAction::Delete),
            _ => Err(ParseEnumError(String::from("YammerAction"))),
        }
    }
}

impl From<YammerAction> for &str {
    fn from(value: YammerAction) -> Self {
        match value {
            YammerAction::List => "List",
            YammerAction::Delete => "Delete",
        }
    }
}

impl From<YammerAction> for usize {
    fn from(value: YammerAction) -> Self {
        match value {
            YammerAction::List => 0,
            YammerAction::Delete => 1,
        }
    }
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
