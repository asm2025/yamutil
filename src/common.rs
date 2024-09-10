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
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

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
