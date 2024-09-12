mod action_handler;
mod app;
mod common;
mod error;
mod service;

use action_handler::ActionHandler;
use chrono::Local;
use clap::Parser;
use dotenv::dotenv;
use log::{error, info};
use rustmix::{
    error::*,
    log4rs::{
        self,
        config::{runtime::Config, Logger, Root},
    },
    *,
};
use std::sync::Arc;

use crate::{app::*, common::*, service::*};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Called first to set debug flag. It affects the log level
    let args = Args::parse();
    set_debug(args.debug);

    let gaurd = log4rs::from_config(configure_log()?)?;
    info!("{} v{} started", APP_INFO.name, APP_INFO.version);

    let service = Arc::new(Service::new());

    if let Some(action) = args.action {
        let handler = ActionHandler::new(service.clone());
        match handler.process(&action).await {
            Ok(_) => {}
            Err(e) => {
                error!("{}", e.get_message());
            }
        };
        return Ok(());
    }

    let app = App::new(APP_INFO.clone(), service.clone());
    match app.run().await {
        Ok(_) => {}
        Err(e) => {
            error!("{}", e.get_message());
        }
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
        LOGDIR.join(Local::now().format("yam-%Y%m%d.log").to_string()),
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
