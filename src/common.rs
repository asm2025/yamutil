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
