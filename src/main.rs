mod common;
mod errors;
mod operations;

use chrono::Local;
use dotenv::dotenv;
use humantime::format_duration;
use log::{error, info, warn};
use rand::{seq::SliceRandom, thread_rng};
use rustmix::{
    error::ErrorEx,
    io::{
        file::{self, FileEx},
        path::PathEx,
    },
    log4rs::{
        self,
        append::file::FileAppender,
        config::{
            runtime::{Config, ConfigBuilder},
            Appender, Logger, Root,
        },
        encode::pattern::PatternEncoder,
    },
    sound::{Audio, WhisperSource},
    threading::{Consumer, ConsumerOptions, Spinner},
    vpn::{ExpressVPN, ExpressVPNStatus},
    *,
};
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time,
};
use structopt::{clap::Shell, StructOpt};
use tempfile::NamedTempFile;

use common::*;
use operations::*;

const VPN_ROTATION_MIN: u64 = 5;
const VPN_ROTATION_MAX: u64 = 1440;
const PROXY_ROTATION_MIN: u64 = 60;
const PROXY_ROTATION_MAX: u64 = 1440;

#[derive(Debug, StructOpt)]
#[structopt(
    name = env!("CARGO_PKG_NAME"),
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS"),
    about = env!("CARGO_PKG_DESCRIPTION")
)]
struct Args {
    #[structopt(
        short,
        long,
        help = r"Number of threads to be used.
Default is 1 when debug mode is on,
otherwise the number of Cores."
    )]
    threads: Option<usize>,
    #[structopt(
        short = "n",
        long,
        help = r"Randomize numbers entries.
in/numbers.txt should be configured."
    )]
    randomize_numbers: bool,
    #[structopt(
        short,
        long,
        help = r"Use ExpressVPN to change the IP address.
in/vpn.txt should be configured
and ExpressVPN must be activated."
    )]
    vpn: bool,
    #[structopt(
        short = "o",
        long,
        help = r"Randomize VPN entries.
in/vpn.txt should be configured
and vpn must be enabled."
    )]
    randomize_vpn: bool,
    #[structopt(
        short = "p",
        long,
        help = r"The duration of which VPN will be rotated in minutes.
in/vpn.txt should be configured
and vpn must be enabled."
    )]
    vpn_rotation: Option<u64>,
    #[structopt(
        short = "x",
        long,
        help = r"Use proxies from the proxies file.
in/proxies.txt should be configured."
    )]
    proxy: bool,
    #[structopt(
        short = "m",
        long,
        help = r"Randomize proxies entries.
in/proxies.txt should be configured
and proxy must be enabled."
    )]
    randomize_proxies: bool,
    #[structopt(
        short = "r",
        long,
        help = r"The duration of which proxies will be rotated in minutes.
in/proxies.txt should be configured
and proxy must be enabled."
    )]
    proxies_rotation: Option<u64>,
    #[structopt(
        short,
        long,
        help = r"Enable debug mode to save responses to files
Threads must be 1 and the build is a debug build."
    )]
    debug: bool,
    #[structopt(
        short = "a",
        long,
        help = r"Test transcribing audio files.
Debug mode must be enabled."
    )]
    test_transcribtion: bool,
    #[structopt(
        short,
        long,
        help = r"Apply burp suite certificate.
Debug mode must be enabled.
i.e. /etc/ssl/certs/portSwagger.pem
i.e. C:\certs\portSwagger.crt",
        parse(from_os_str)
    )]
    burp: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    Args::clap().gen_completions(env!("CARGO_PKG_NAME"), Shell::Bash, "./");

    // Called first to set debug flag. It affects the log level
    let args = Args::from_args();
    set_debug(args.debug);
    let gaurd = log4rs::from_config(configure_log()?)?;

    output::print_header(&APP_INFO);
    info!("{} v{} started", APP_INFO.name, APP_INFO.version);
    println!(
        r"If this is the first time to run the application, it will download
the audio model and tokenizer files. It could take a few seconds/minutes
to initialize it. So have patience and wait for the model initialized message."
    );

    let max_threads = num_cpus();
    let threads = args.threads.unwrap_or(max_threads).clamp(1, max_threads);
    info!("Using {} thread(s)", threads);

    let spinner = Spinner::new();
    spinner.set_message("Initializing audio model");
    let audio = match Audio::with_source(WhisperSource::DistilLargeV3).await {
        Ok(it) => it,
        Err(e) => {
            error!("{}", e.get_message());
            return Ok(());
        }
    };
    spinner.finish_with_message("Audio model initialized")?;

    if args.test_transcribtion {
        if !is_debug() {
            error!("Debug mode must be enabled to test audio transcribtion");
            return Ok(());
        }

        match test_transcribtion(&audio, &spinner).await {
            Ok(_) => (),
            Err(e) => {
                error!("{}", e.get_message());
                return Ok(());
            }
        }

        return Ok(());
    }

    match set_ca(args.burp.clone()) {
        Ok(_) => (),
        Err(e) => {
            error!("{}", e.get_message());
            return Ok(());
        }
    }

    let vpn = if args.vpn {
        match get_vpn() {
            Ok(it) => it,
            Err(e) => {
                error!("{}", e.get_message());
                return Ok(());
            }
        }
    } else {
        None
    };
    let locations = if vpn.is_some() {
        match read_vpn_locations() {
            Ok(it) => it,
            Err(e) => {
                error!("{}", e.get_message());
                return Ok(());
            }
        }
    } else {
        Vec::with_capacity(0)
    };
    let proxies = if args.proxy {
        match read_proxies(threads) {
            Ok(it) => it,
            Err(e) => {
                error!("{}", e.get_message());
                return Ok(());
            }
        }
    } else {
        Vec::with_capacity(0)
    };
    let bad_ua = match read_bad_ua() {
        Ok(it) => it,
        Err(e) => {
            error!("{}", e.get_message());
            return Ok(());
        }
    };
    info!("Initializing task handler");

    let handler = {
        let vpn_rotation = match args.vpn_rotation {
            Some(it) => {
                if it == 0 {
                    0
                } else {
                    it.clamp(VPN_ROTATION_MIN, VPN_ROTATION_MAX)
                }
            }
            None => 0,
        };
        let proxies_rotation = match args.proxies_rotation {
            Some(it) => {
                if it == 0 {
                    0
                } else {
                    it.clamp(PROXY_ROTATION_MIN, PROXY_ROTATION_MAX)
                }
            }
            None => 0,
        };

        if locations.is_empty() {
            info!("Not using VPN");
        } else if args.randomize_vpn {
            info!("Using random VPN");
        } else {
            info!("Using VPN");
        }

        if proxies.is_empty() {
            info!("Not using proxies");
        } else if args.randomize_proxies {
            info!("Using random proxies");
        } else {
            info!("Using proxies");
        }

        match TaskHandler::new(
            threads,
            audio,
            vpn,
            locations,
            args.randomize_vpn,
            vpn_rotation * 60u64,
            proxies,
            args.randomize_proxies,
            proxies_rotation * 60u64,
            bad_ua,
        ) {
            Ok(it) => it,
            Err(e) => {
                error!("{}", e.get_message());
                return Ok(());
            }
        }
    };
    let options = ConsumerOptions::new();
    let consumer = Consumer::<String>::with_options(options);

    info!("Starting consumers");
    match consumer.start(&handler) {
        Ok(_) => (),
        Err(e) => {
            error!("{}", e.get_message());
            return Ok(());
        }
    }

    match queue_numbers(&consumer, args.randomize_numbers) {
        Ok(_) => (),
        Err(e) => {
            error!("{}", e.get_message());
            return Ok(());
        }
    }

    info!("Waiting for tasks to complete");
    match consumer.wait_async().await {
        Ok(_) => (),
        Err(e) => {
            error!("{}", e.get_message());
            return Ok(());
        }
    }
    info!("Tasks completed");

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
    let mut logger = log4rs::configure(
        LOGDIR.join(Local::now().format("fb-%Y%m%d.log").to_string()),
        log_level,
        None,
    )?
    .logger(Logger::builder().build("selectors", log::LevelFilter::Warn))
    .logger(Logger::builder().build("html5ever", log::LevelFilter::Warn))
    .logger(Logger::builder().build("hyper_util", log::LevelFilter::Warn))
    .logger(Logger::builder().build("tokenizers", log::LevelFilter::Error))
    .logger(Logger::builder().build("symphonia_core", log::LevelFilter::Warn));
    logger = configure_log_numbers(logger, NUMBERS_GOOD, OUTDIR.join(FILE_NUMBERS_GOOD))?;
    logger = configure_log_numbers(logger, NUMBERS_BAD, OUTDIR.join(FILE_NUMBERS_BAD))?;
    logger = configure_log_ua(logger, UA_BAD, INDIR.join(FILE_UA_BAD))?;
    let config = logger.build(
        Root::builder()
            .appender("console")
            .appender("file")
            .build(log_level.into()),
    )?;
    Ok(config)
}

fn configure_log_numbers<T: AsRef<Path>>(
    config: ConfigBuilder,
    name: &str,
    file_name: T,
) -> Result<ConfigBuilder> {
    let file = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{m}{n}")))
        .append(true)
        .build(file_name)?;
    Ok(config
        .appender(Appender::builder().build(name, Box::new(file)))
        .logger(
            Logger::builder()
                .appender(name)
                .build(name, log::LevelFilter::Trace),
        ))
}

fn configure_log_ua<T: AsRef<Path>>(
    config: ConfigBuilder,
    name: &str,
    file_name: T,
) -> Result<ConfigBuilder> {
    let file = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{m}{n}")))
        .append(true)
        .build(file_name)?;
    Ok(config
        .appender(Appender::builder().build(name, Box::new(file)))
        .logger(
            Logger::builder()
                .appender(name)
                .build(name, log::LevelFilter::Trace),
        ))
}

fn set_ca(path: Option<PathBuf>) -> Result<()> {
    if let Some(path) = path {
        if !path.is_file() {
            return Err(format!("CA certificate '{}' does not exist", &path.display()).into());
        }

        set_burp_cert(path.clone());
        info!("Using CA certificate '{}'", &path.display());
    }

    Ok(())
}

fn check_file(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        return Err(format!("File '{}' does not exist.", &path.display()).into());
    }

    let metadata = fs::metadata(&path)?;

    if metadata.len() == 0 {
        return Err(format!("File '{}' is empty.", &path.display()).into());
    }

    Ok(())
}

fn queue_numbers(consumer: &Consumer<String>, shuffle: bool) -> Result<()> {
    let path = INDIR.join(FILE_NUMBERS);

    if let Err(e) = check_file(&path) {
        return Err(e);
    }

    info!("Reading numbers from '{}'", &path.display());
    let mut file = match file::open(&path) {
        Ok(it) => it,
        Err(e) => {
            return Err(e);
        }
    };

    if shuffle {
        let reader = BufReader::new(file);
        let mut lines: Vec<String> = reader
            .lines()
            .map(|it| it.unwrap())
            .filter(|it| !it.is_empty())
            .collect();

        let mut rng = thread_rng();
        lines.shuffle(&mut rng);

        let mut temp_file = NamedTempFile::new()?;

        for line in &lines {
            writeln!(temp_file, "{}", line)?;
        }

        fs::rename(temp_file.path(), &path)?;
        file = match file::open(&path) {
            Ok(it) => it,
            Err(e) => {
                return Err(e);
            }
        };
    }

    for number in match file.read() {
        Ok(it) => it,
        Err(e) => {
            return Err(e);
        }
    } {
        consumer.enqueue(number)?;
    }

    Ok(())
}

fn get_vpn() -> Result<Option<ExpressVPN>> {
    let vpn = ExpressVPN;
    let version = match vpn.version() {
        Ok(it) => it,
        Err(e) => {
            warn!("Could not get Express VPN version.");
            let confirmed = match input::confirm(&format!(
                "{} Do you want to continue? [y] ",
                &e.to_string()
            )) {
                Ok(c) => c,
                _ => false,
            };

            if !confirmed {
                return Err(e);
            }

            return Ok(None);
        }
    };
    info!("Using ExpressVPN version {}", &version);
    let status = vpn.status()?;
    match status {
        ExpressVPNStatus::Connected(_) => {
            vpn.disconnect()?;
        }
        ExpressVPNStatus::Error(e) => {
            return Err(e.into());
        }
        ExpressVPNStatus::NotActivated => {
            return Err("VPN not activated".into());
        }
        ExpressVPNStatus::Unknown => {
            return Err("Unknow VPN status".into());
        }
        _ => {}
    }

    vpn.network_lock(true)?;
    Ok(Some(vpn))
}

fn read_vpn_locations() -> Result<Vec<String>> {
    const MAX_LOC: usize = 10000;

    let path = INDIR.join(FILE_VPN);

    if let Err(e) = check_file(&path) {
        let confirmed =
            match input::confirm(&format!("{} Do you want to continue? [y] ", &e.to_string())) {
                Ok(c) => c,
                _ => false,
            };

        if !confirmed {
            return Err(e);
        }
    }

    let file = file::open(&path)?;
    let mut locations = Vec::with_capacity(MAX_LOC);

    for location in file.read()?.take(MAX_LOC) {
        locations.push(location);
    }

    if locations.is_empty() {
        let msg = format!("No VPNs found in '{}'", &path.display());
        let confirmed = match input::confirm(&format!("{} Do you want to continue? [y] ", &msg)) {
            Ok(c) => c,
            _ => false,
        };

        if !confirmed {
            return Err(msg.into());
        }
    }

    Ok(locations)
}

fn read_proxies(threads: usize) -> Result<Vec<String>> {
    const MAX_PROXIES: usize = 1_000_000;

    let path = INDIR.join(FILE_PROXIES);

    if let Err(e) = check_file(&path) {
        let confirmed =
            match input::confirm(&format!("{} Do you want to continue? [y] ", &e.to_string())) {
                Ok(c) => c,
                _ => false,
            };

        if !confirmed {
            return Err(e);
        }
    }

    let file = file::open(&path)?;
    let mut proxies = Vec::with_capacity(threads);

    for proxy in file.read()?.take(MAX_PROXIES) {
        proxies.push(proxy);
    }

    if proxies.is_empty() {
        let msg = format!("No proxies found in '{}'", &path.display());
        let confirmed = match input::confirm(&format!("{} Do you want to continue? [y] ", &msg)) {
            Ok(c) => c,
            _ => false,
        };

        if !confirmed {
            return Err(msg.into());
        }
    }

    Ok(proxies)
}

fn read_bad_ua() -> Result<HashSet<String>> {
    const MAX_UA: usize = 1_000_000;

    let mut bad_ua = HashSet::new();
    let path = INDIR.join(FILE_UA_BAD);

    if check_file(&path).is_err() {
        return Ok(bad_ua);
    }

    let file = file::open(&path)?;

    for ua in file.read()?.take(MAX_UA) {
        bad_ua.insert(ua.to_lowercase());
    }

    Ok(bad_ua)
}

async fn test_transcribtion(sound: &Audio, spinner: &Spinner) -> Result<()> {
    if !is_debug() {
        return Ok(());
    }

    let audiodir = CURDIR.join("test/audio");

    let file_name = audiodir.join("awz1.mp3");
    let base_name = file_name.file_name().unwrap().to_string_lossy().to_string();
    spinner.reset()?;
    spinner.set_message(format!("Transcribing file [text]: {}", &base_name));
    let snd = sound.clone();
    let timer = time::Instant::now();
    let result = spinner.run(move || snd.transcribe_file(file_name).unwrap())?;
    spinner.finish_with_message(format!(
        "Sound transcription [{}]: '{}'",
        &base_name, result
    ))?;
    println!("Time elapsed: {}", format_duration(timer.elapsed()));

    let file_name = audiodir.join("fb1.mp3");
    let base_name = file_name.file_name().unwrap().to_string_lossy().to_string();
    spinner.reset()?;
    spinner.set_message(format!("Transcribing file [text]: {}", &base_name));
    let snd = sound.clone();
    let timer = time::Instant::now();
    let result = spinner.run(move || snd.transcribe_file(&file_name).unwrap())?;
    spinner.finish_with_message(format!(
        "Sound transcription [{}]: '{}'",
        &base_name, result
    ))?;
    println!("Time elapsed: {}", format_duration(timer.elapsed()));

    let file_name = audiodir.join("pinless.wav");
    let base_name = file_name.file_name().unwrap().to_string_lossy().to_string();
    spinner.reset()?;
    spinner.set_message(format!("Transcribing file [text]: {}", &base_name));
    let snd = sound.clone();
    let timer = time::Instant::now();
    let result = spinner.run(move || snd.transcribe_file(&file_name).unwrap())?;
    spinner.finish_with_message(format!(
        "Sound transcription [{}]: '{}'",
        &base_name, result
    ))?;
    println!("Time elapsed: {}", format_duration(timer.elapsed()));

    Ok(())
}
