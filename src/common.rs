use humantime::format_duration;
use lazy_static::lazy_static;
use log::{info, warn};
use rustmix::{
    io::directory,
    random,
    web::{reqwest::blocking::Client as BlockingClient, *},
    AppInfo, Result,
};
use std::{fmt, path::PathBuf, thread, time::Duration};

pub const FILE_VPN: &'static str = "vpn.txt";
pub const FILE_PROXIES: &'static str = "proxies.txt";
pub const FILE_NUMBERS: &'static str = "numbers.txt";
pub const FILE_NUMBERS_GOOD: &'static str = "numbers_ok.txt";
pub const FILE_NUMBERS_BAD: &'static str = "numbers_bad.txt";
pub const FILE_UA_BAD: &'static str = "ua_bad.txt";
pub const NUMBERS_GOOD: &'static str = "numbers_ok";
pub const NUMBERS_BAD: &'static str = "numbers_bad";
pub const UA_BAD: &'static str = "ua_bad";
pub const BASE_URL: &'static str = "https://mbasic.facebook.com/";
pub const CAPTCHA_LEN: usize = 6;
pub const RETRY_3: usize = 3;
pub const RETRY_5: usize = 5;
pub const RETRY_10: usize = RETRY_5 * 2;
pub const RETRY_25: usize = RETRY_5 * 5;
pub const RETRY_100: usize = RETRY_25 * 4;
pub const INVITATIONS: usize = RETRY_10;

lazy_static! {
    pub static ref APP_INFO: AppInfo<'static> = AppInfo::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
        Some(env!("CARGO_PKG_DESCRIPTION")),
        Some(env!("CARGO_PKG_LICENSE")),
    );
    pub static ref CURDIR: PathBuf = directory::current();
    pub static ref INDIR: PathBuf = CURDIR.join("in");
    pub static ref OUTDIR: PathBuf = CURDIR.join("out");
    pub static ref LOGDIR: PathBuf = CURDIR.join("_logs");
    pub static ref TMPDIR: PathBuf = CURDIR.join("tmp");
    static ref WAIT_TIME: Duration = Duration::from_secs(2);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Gender {
    Female,
    Male,
}

impl Gender {
    pub fn random() -> Self {
        match random::boolean() {
            false => Gender::Female,
            _ => Gender::Male,
        }
    }
}

impl fmt::Display for Gender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Gender::Female => write!(f, "female"),
            Gender::Male => write!(f, "male"),
        }
    }
}

impl From<Gender> for &str {
    fn from(value: Gender) -> Self {
        match value {
            Gender::Female => "Female",
            Gender::Male => "Male",
        }
    }
}

impl From<Gender> for usize {
    fn from(value: Gender) -> Self {
        match value {
            Gender::Female => 1,
            Gender::Male => 2,
        }
    }
}

#[cfg(debug_assertions)]
pub const TIMEOUT: u64 = 30;
#[cfg(not(debug_assertions))]
pub const TIMEOUT: u64 = 5;

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

pub fn print_ip(client: &BlockingClient) -> Result<()> {
    let _tries = RETRY_3;
    let mut tries = 0;
    info!("fetching IP address");

    while _tries > tries {
        match get_public_ip(client) {
            Ok(it) => {
                info!("Current IP address {}", it);
                break;
            }
            Err(e) => {
                tries += 1;
                if _tries > tries {
                    warn!("No connection! Waiting for {}", format_duration(*WAIT_TIME));
                    thread::sleep(*WAIT_TIME);
                    continue;
                }
                return Err(e.into());
            }
        };
    }

    Ok(())
}
