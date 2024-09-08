mod common;
mod errors;

use chrono::Local;
use dotenv::dotenv;
use humantime::format_duration;
use log::{error, info, warn};
use rand::{seq::SliceRandom, thread_rng};
use rustmix::{
    io::path::PathEx,
    log4rs::{
        self,
        config::{runtime::Config, Logger, Root},
    },
    *,
};
use std::time;
use structopt::StructOpt;

use common::*;

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
    email: String,
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

    info!("Shutting down");
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
