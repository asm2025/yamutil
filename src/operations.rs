use crossbeam_skiplist::SkipMap;
use humantime::format_duration;
use lazy_static::lazy_static;
use log::{debug, error, info, warn};
use mime::Mime;
use rand::{seq::SliceRandom, thread_rng, Rng};
use reqwest_cookie_store::{CookieStore, CookieStoreRwLock};
use rustmix::{
    error::*,
    io::{
        directory,
        file::{create_with, FileOpenOptions},
        path::{self, PathEx},
    },
    random,
    sound::*,
    threading::{Consumer, TaskDelegation, TaskResult},
    vpn::ExpressVPN,
    web::{
        reqwest::{
            blocking::{Client, ClientBuilder},
            build_blocking_client_with_user_agent,
            header::{self, HeaderMap, HeaderValue},
            redirect, Certificate, Proxy,
        },
        AsUrl,
    },
    *,
};
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::{
    cmp,
    collections::HashSet,
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant},
};
use urlencoding::decode;

use crate::{common::*, errors::*};

lazy_static! {
    static ref CAPATH: Mutex<Option<PathBuf>> = Mutex::new(None);
    static ref SLEEP_QUEUE: Duration = Duration::from_secs(5);
    static ref SLEEP_LIMIT: Duration = Duration::from_secs(120);
}

const LIMIT_TEXT: &'static str = "limit how often you can post";
const SMS_SUCCESS_TEXT: &'static str = "Enter the code we sent by text";

pub fn set_burp_cert(value: PathBuf) {
    let mut capath = CAPATH.lock().unwrap();
    *capath = Some(value);
}

#[derive(Debug, Clone)]
pub(crate) struct TaskHandler {
    threads: Arc<AtomicUsize>,
    started: Arc<Mutex<Option<Instant>>>,
    clients: Arc<RwLock<Vec<(Arc<Client>, Arc<CookieStoreRwLock>)>>>,
    vpn: Arc<Option<ExpressVPN>>,
    locations: Arc<Mutex<Vec<String>>>,
    locations_used: Arc<Mutex<Vec<String>>>,
    vpn_random: bool,
    // time to rotate VPN in seconds. 0 means no rotation.
    vpn_rotation: u64,
    proxies: Arc<Mutex<Vec<String>>>,
    proxies_used: Arc<Mutex<Vec<String>>>,
    proxy_random: bool,
    // time to rotate proxies in seconds. 0 means no rotation.
    proxies_rotation: u64,
    repeat: Arc<SkipMap<String, usize>>,
    bad_ua: Arc<RwLock<HashSet<String>>>,
    audio: Arc<Audio>,
    timer: Arc<Mutex<Option<Instant>>>,
    save_responses: Arc<AtomicBool>,
}

impl TaskHandler {
    pub fn new(
        threads: usize,
        audio: Audio,
        vpn: Option<ExpressVPN>,
        loctions: Vec<String>,
        vpn_random: bool,
        vpn_rotation: u64,
        proxies: Vec<String>,
        proxy_random: bool,
        proxies_rotation: u64,
        bad_ua: HashSet<String>,
    ) -> Result<Self> {
        if threads == 0 {
            panic!("Threads must be greater than 0");
        }
        Ok(Self {
            threads: Arc::new(AtomicUsize::new(threads)),
            started: Arc::new(Mutex::new(None)),
            clients: Arc::new(RwLock::new(Vec::with_capacity(threads))),
            vpn: Arc::new(vpn),
            locations: Arc::new(Mutex::new(loctions)),
            locations_used: Arc::new(Mutex::new(Vec::with_capacity(0))),
            vpn_random,
            vpn_rotation,
            proxies: Arc::new(Mutex::new(proxies)),
            proxies_used: Arc::new(Mutex::new(Vec::with_capacity(0))),
            proxy_random,
            proxies_rotation,
            repeat: Arc::new(SkipMap::new()),
            bad_ua: Arc::new(RwLock::new(bad_ua)),
            audio: Arc::new(audio),
            timer: Arc::new(Mutex::new(None)),
            save_responses: Arc::new(AtomicBool::new(threads == 1 && is_debug())),
        })
    }

    fn stp1_get_payload(
        &self,
        pc: &Consumer<String>,
        client: &Client,
        item: &String,
    ) -> Result<(TaskResult, Option<Value>)> {
        let url = (BASE_URL, "reg/?cid=103&refsrc=deprecated&_rdr").as_url()?;
        let _enqueued_times = RETRY_10;
        let _tries = RETRY_3;
        let mut tries = 0;

        while _tries > tries {
            tries += 1;
            self.print_step(1, &tries, &item);

            let response = match client.post(url.clone()).send() {
                Ok(it) => {
                    if it.status().is_success() {
                        it
                    } else {
                        warn!("{} -> Invalid response {}", &item, &it.status());
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(it.error_for_status().unwrap_err().get_message()),
                            None,
                        ));
                    }
                }
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };
            let text = match response.text() {
                Ok(it) => it,
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            if text.is_empty() {
                if _tries > tries {
                    continue;
                }
                return Ok((TaskResult::Error(NoContentError.get_message()), None));
            }

            if self.save_responses() {
                self.save_request(1, None, &text)?;
            }

            if text.contains(LIMIT_TEXT) {
                let enqueued_times = self.repeat.get(item.as_str()).map_or(1, |e| *e.value() + 1);

                if _enqueued_times > enqueued_times {
                    self.repeat.insert(item.clone(), enqueued_times);
                    let msg = format!("Limit block. Enqueue [{}] item {}", &enqueued_times, &item);
                    warn!("{}", &msg);
                    info!("Sleeping for {}", format_duration(*SLEEP_LIMIT));
                    thread::sleep(*SLEEP_LIMIT);
                    pc.enqueue(item.clone())?;
                    return Ok((TaskResult::Error(msg), None));
                }

                return Ok((
                    TaskResult::Error(BlockedRequestLimitError.get_message()),
                    None,
                ));
            }

            let document = Html::parse_document(&text);
            let selector = Selector::parse("input[name=lsd]")?;
            // check the first element only. the rest will be likely there if the first one is there
            let lsd = match document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=lsd]".to_string()))
            {
                Ok(it) => match it.value().attr("value") {
                    Some(it) => it.to_string(),
                    None => {
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(
                                ElementNotFoundError("input[name=lsd]".to_string()).get_message(),
                            ),
                            None,
                        ));
                    }
                },
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);
                    if _tries > tries {
                        continue;
                    }
                    return Ok((TaskResult::Error(msg), None));
                }
            };
            let selector = Selector::parse("input[name=jazoest]")?;
            let jazoest = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=jazoest]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("input[name=reg_instance]")?;
            let reg_instance = match document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=reg_instance]".to_string()))
            {
                Ok(it) => match it.value().attr("value") {
                    Some(it) => it.to_string(),
                    None => {
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(
                                ElementNotFoundError("value".to_string()).get_message(),
                            ),
                            None,
                        ));
                    }
                },
                Err(e) => {
                    let msg = e.get_message();
                    let enqueued_times =
                        self.repeat.get(item.as_str()).map_or(1, |e| *e.value() + 1);

                    if _enqueued_times > enqueued_times {
                        self.repeat.insert(item.clone(), enqueued_times);
                        let msg = format!(
                            "{}. Enqueue [{}] item {}",
                            e.get_message(),
                            &enqueued_times,
                            &item
                        );
                        warn!("{}", msg);

                        if pc.len() + pc.running() < num_cpus() {
                            info!("Sleeping for {}", format_duration(*SLEEP_QUEUE));
                            thread::sleep(*SLEEP_QUEUE);
                        }

                        pc.enqueue(item.clone())?;
                        return Ok((TaskResult::Error(msg), None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };
            let selector = Selector::parse("input[name=reg_impression_id]")?;
            let reg_impression_id = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=reg_impression_id]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let payload = json!({
                "lsd": lsd,
                "jazoest": jazoest,
                "ccp": 2,
                "reg_instance": reg_instance,
                "reg_impression_id": reg_impression_id,
                "submission_request": true,
                "helper": "",
                "ns": 0,
                "zero_header_af_client": "",
                "app_id": "",
                "logger_id": "",
                "field_names[]": "reg_passwd__",
                "firstname": random::person::first_name(),
                "lastname": random::person::last_name(),
                "reg_email__": random::internet::free_email(),
                "sex": Gender::random() as usize,
                "custom_gender": "",
                "did_use_age": false,
                "birthday_month": random::numeric(1..12).to_string(),
                "birthday_day": random::numeric(1..28).to_string(),
                "birthday_year": random::numeric(1970..2006).to_string(),
                "age_step_input": "",
                "reg_passwd__": random::internet::password(8..16),
                "submit": "Sign+Up"
            });

            return Ok((TaskResult::Success, Some(payload)));
        }

        Ok((TaskResult::Error(MaxTriesExceededError.get_message()), None))
    }

    fn stp2_post_payload(
        &self,
        pc: &Consumer<String>,
        client: &Client,
        cookies: &CookieStoreRwLock,
        item: &String,
        payload: Value,
    ) -> Result<(TaskResult, Option<(String, Value)>)> {
        let prevurl = format!("{}reg/?cid=103&refsrc=deprecated&_rdr", BASE_URL);
        let url = (BASE_URL, "reg/?cid=103").as_url()?;
        let _enqueued_times = RETRY_10;
        let _tries = RETRY_3;
        let mut tries = 0;

        while _tries > tries {
            tries += 1;
            self.print_step(2, &tries, &item);

            let response = match client
                .post(url.clone())
                .header(header::REFERER, HeaderValue::from_str(&prevurl)?)
                .form(&payload)
                .send()
            {
                Ok(it) => {
                    if it.status().is_success() {
                        it
                    } else {
                        warn!("{} -> Invalid response {}", &item, &it.status());
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(it.error_for_status().unwrap_err().get_message()),
                            None,
                        ));
                    }
                }
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            if cookies.read().unwrap().iter_any().any(|c| c.name() == "rs") {
                let enqueued_times = self.repeat.get(item.as_str()).map_or(1, |e| *e.value() + 1);

                if _enqueued_times > enqueued_times {
                    self.repeat.insert(item.clone(), enqueued_times);
                    let msg = format!(
                        "Request blocked by cookie. Enqueue [{}] item {}",
                        &enqueued_times, &item
                    );
                    warn!("{}", msg);
                    pc.enqueue(item.clone())?;
                    return Ok((TaskResult::Error(msg), None));
                }

                return Err(BlockedRequestError.into());
            }

            let text = match response.text() {
                Ok(it) => it,
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            if text.is_empty() {
                if _tries > tries {
                    continue;
                }
                return Ok((TaskResult::Error(NoContentError.get_message()), None));
            }

            if self.save_responses() {
                self.save_request(2, None, &text)?;
            }

            let document = Html::parse_document(&text);
            let selector = Selector::parse("form")?;
            // check the first element only. the rest will be likely there if the first one is there
            let action = match document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("form".to_string()))
            {
                Ok(it) => match it.value().attr("action") {
                    Some(it) => it.to_string(),
                    None => {
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(
                                ElementNotFoundError("action".to_string()).get_message(),
                            ),
                            None,
                        ));
                    }
                },
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);
                    if _tries > tries {
                        continue;
                    }
                    return Ok((TaskResult::Error(msg), None));
                }
            };
            let action = (BASE_URL, action.as_str()).as_url()?.to_string();
            if action.len() < 200 {
                return Err(InvalidFormActionError(action).into());
            }
            let selector = Selector::parse("input[name=fb_dtsg]")?;
            let fb_dtsg = match document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=fb_dtsg]".to_string()))
            {
                Ok(it) => match it.value().attr("value") {
                    Some(it) => it.to_string(),
                    None => {
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(
                                ElementNotFoundError("value".to_string()).get_message(),
                            ),
                            None,
                        ));
                    }
                },
                Err(e) => {
                    let enqueued_times =
                        self.repeat.get(item.as_str()).map_or(1, |e| *e.value() + 1);

                    if _enqueued_times > enqueued_times {
                        self.repeat.insert(item.clone(), enqueued_times);
                        let msg = format!(
                            "Request blocked. Enqueue [{}] item {}",
                            &enqueued_times, &item
                        );
                        warn!("{}", msg);

                        if pc.len() + pc.running() < num_cpus() {
                            info!("Sleeping for {}", format_duration(*SLEEP_QUEUE));
                            thread::sleep(*SLEEP_QUEUE);
                        }

                        pc.enqueue(item.clone())?;
                        return Ok((TaskResult::Error(msg), None));
                    }

                    return Ok((TaskResult::Error(e.get_message()), None));
                }
            };
            let selector = Selector::parse("input[name=jazoest]")?;
            let jazoest = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=jazoest]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("input[name=action_proceed]")?;
            let action_proceed = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=action_proceed]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let payload = json!({
                "fb_dtsg": fb_dtsg,
                "jazoest": jazoest,
                "action_proceed": action_proceed,
            });

            return Ok((TaskResult::Success, Some((action, payload))));
        }

        Ok((TaskResult::Error(MaxTriesExceededError.get_message()), None))
    }

    fn stp3_post_form_action(
        &self,
        pc: &Consumer<String>,
        client: &Client,
        item: &String,
        url: &String,
        payload: Value,
    ) -> Result<(TaskResult, Option<(String, Value)>)> {
        let _enqueued_times = RETRY_10;
        let _tries = RETRY_3;
        let mut tries = 0;

        while _tries > tries {
            tries += 1;
            self.print_step(3, &tries, &item);

            let response = match client
                .post(url.clone())
                .header(header::REFERER, HeaderValue::from_str(url)?)
                .form(&payload)
                .send()
            {
                Ok(it) => {
                    if it.status().is_success() {
                        it
                    } else {
                        warn!("{} -> Invalid response {}", &item, &it.status());
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(it.error_for_status().unwrap_err().get_message()),
                            None,
                        ));
                    }
                }
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            let text = match response.text() {
                Ok(it) => it,
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            if text.is_empty() {
                if _tries > tries {
                    continue;
                }
                return Ok((TaskResult::Error(NoContentError.get_message()), None));
            }

            if self.save_responses() {
                self.save_request(3, None, &text)?;
            }

            let document = Html::parse_document(&text);
            let selector = Selector::parse("form[id^='root_']")?;
            // check the first element only. the rest will be likely there if the first one is there
            let action = match document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("form[id^='root_']".to_string()))
            {
                Ok(it) => match it.value().attr("action") {
                    Some(it) => it.to_string(),
                    None => {
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(
                                ElementNotFoundError("action".to_string()).get_message(),
                            ),
                            None,
                        ));
                    }
                },
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);
                    if _tries > tries {
                        continue;
                    }
                    return Ok((TaskResult::Error(msg), None));
                }
            };
            let action = (BASE_URL, action.as_str()).as_url()?.to_string();
            if action.len() < 200 {
                return Err(InvalidFormActionError(action).into());
            }
            let selector = Selector::parse("input[name=fb_dtsg]")?;
            let fb_dtsg = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=fb_dtsg]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("input[name=jazoest]")?;
            let jazoest = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=jazoest]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("input[name=captcha_persist_data]")?;
            let captcha_persist_data = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=captcha_persist_data]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("input[name=action_submit_bot_captcha_response]")?;
            let action_submit_bot_captcha_response = document
                .select(&selector)
                .next()
                .ok_or_else(|| {
                    ElementNotFoundError(
                        "input[name=action_submit_bot_captcha_response]".to_string(),
                    )
                })
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse(r#"a[onclick^="new Audio("]"#)?;
            let audio_link = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError(r#"a[onclick^="new Audio("]"#.to_string()))
                .unwrap()
                .value()
                .attr("onclick")
                .unwrap()
                .to_string();
            let start = audio_link.find("new Audio(\"").unwrap();
            let end = audio_link.rfind("\")").unwrap();
            let audio_link = decode(&audio_link[start + 11..end].replace("\\", ""))?.to_string();
            let audio_file = match self.download_captcha_audio(&client, &audio_link, &item) {
                Ok(it) => it,
                Err(e) => {
                    return Ok((TaskResult::Error(e.get_message()), None));
                }
            };
            let captcha_response = match self.resolve_captcha(&item, &audio_file) {
                Ok(it) => it,
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);
                    if _tries > tries {
                        continue;
                    }
                    let enqueued_times =
                        self.repeat.get(item.as_str()).map_or(1, |e| *e.value() + 1);

                    if _enqueued_times > enqueued_times {
                        self.repeat.insert(item.clone(), enqueued_times);
                        let msg = format!(
                            "Could not resolve captch audio. Enqueue [{}] item {}",
                            &enqueued_times, &item
                        );
                        warn!("{}", msg);
                        pc.enqueue(item.clone())?;
                        return Ok((TaskResult::Error(msg), None));
                    }

                    return Err(e.into());
                }
            };
            let payload = json!({
                "fb_dtsg": fb_dtsg,
                "jazoest": jazoest,
                "captcha_persist_data": captcha_persist_data,
                "action_submit_bot_captcha_response": action_submit_bot_captcha_response,
                "captcha_response": captcha_response
            });

            return Ok((TaskResult::Success, Some((action, payload))));
        }

        Ok((TaskResult::Error(MaxTriesExceededError.get_message()), None))
    }

    fn stp4_post_captcha_response(
        &self,
        pc: &Consumer<String>,
        client: &Client,
        item: &String,
        url: &String,
        payload: Value,
    ) -> Result<(TaskResult, Option<(String, Value)>)> {
        let _enqueued_times = RETRY_10;
        let _tries = RETRY_3;
        let mut tries = 0;

        while _tries > tries {
            tries += 1;
            self.print_step(4, &tries, &item);

            let response = match client
                .post(url.clone())
                .header(header::REFERER, HeaderValue::from_str(url)?)
                .form(&payload)
                .send()
            {
                Ok(it) => {
                    if it.status().is_success() {
                        it
                    } else {
                        warn!("{} -> Invalid response {}", &item, &it.status());
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(it.error_for_status().unwrap_err().get_message()),
                            None,
                        ));
                    }
                }
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            let text = match response.text() {
                Ok(it) => it,
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            if text.is_empty() {
                if _tries > tries {
                    continue;
                }
                return Ok((TaskResult::Error(NoContentError.get_message()), None));
            }

            if self.save_responses() {
                self.save_request(4, None, &text)?;
            }

            let document = Html::parse_document(&text);
            let selector = Selector::parse("form[id^='root_'] h1[class^='b']:first-of-type")?;
            // check the first element only. the rest will be likely there if the first one is there
            let h1 = match document.select(&selector).next().ok_or_else(|| {
                ElementNotFoundError("form[id^='root_'] h1[class^='b']:first-of-type".to_string())
            }) {
                Ok(it) => it.text().collect::<String>(),
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);
                    if _tries > tries {
                        continue;
                    }
                    return Ok((TaskResult::Error(msg), None));
                }
            };

            if !h1.contains("Add a mobile number or email") {
                let enqueued_times = self.repeat.get(item.as_str()).map_or(1, |e| *e.value() + 1);

                if _enqueued_times > enqueued_times {
                    self.repeat.insert(item.clone(), enqueued_times);
                    let msg = format!(
                        "Mobile number not allowed. Enqueueing item {} [{}]",
                        &item, &enqueued_times
                    );
                    warn!("{}", msg);
                    info!("Sleeping for {}", format_duration(*SLEEP_LIMIT));
                    thread::sleep(*SLEEP_LIMIT);
                    pc.enqueue(item.clone())?;
                    return Ok((
                        TaskResult::Error(MobileNumberNotAllowedError.get_message()),
                        None,
                    ));
                }

                return Err(MobileNumberNotAllowedError.into());
            }

            let selector = Selector::parse("input[name=fb_dtsg]")?;
            let fb_dtsg = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=fb_dtsg]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("input[name=jazoest]")?;
            let jazoest = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=jazoest]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("form[id^='root_']")?;
            let action = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("form[id^='root_']".to_string()))
                .unwrap()
                .value()
                .attr("action")
                .unwrap()
                .to_string();
            let action = (BASE_URL, action.as_str()).as_url()?.to_string();

            if action.len() < 200 {
                return Err(InvalidFormActionError(action).into());
            }

            let payload = json!({
                "fb_dtsg": fb_dtsg,
                "jazoest": jazoest,
                "contact_point": item,
                "action_set_contact_point": "Send login code"
            });
            return Ok((TaskResult::Success, Some((action, payload))));
        }

        Ok((TaskResult::Error(MaxTriesExceededError.get_message()), None))
    }

    fn stp5_add_mobile_number(
        &self,
        _pc: &Consumer<String>,
        client: &Client,
        item: &String,
        url: &String,
        payload: Value,
    ) -> Result<(TaskResult, Option<(String, Value)>)> {
        let _tries = RETRY_3;
        let mut tries = 0;

        while _tries > tries {
            tries += 1;
            self.print_step(5, &tries, &item);

            let response = match client
                .post(url.clone())
                .header(header::REFERER, HeaderValue::from_str(url)?)
                .form(&payload)
                .send()
            {
                Ok(it) => {
                    if it.status().is_success() {
                        it
                    } else {
                        warn!("{} -> Invalid response {}", &item, &it.status());
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(it.error_for_status().unwrap_err().get_message()),
                            None,
                        ));
                    }
                }
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            let text = match response.text() {
                Ok(it) => it,
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);

                    if _tries > tries {
                        continue;
                    }

                    if e.is_timeout() {
                        return Ok((TaskResult::TimedOut, None));
                    }

                    return Ok((TaskResult::Error(msg), None));
                }
            };

            if text.is_empty() {
                if _tries > tries {
                    continue;
                }
                return Ok((TaskResult::Error(NoContentError.get_message()), None));
            }

            if self.save_responses() {
                self.save_request(5, None, &text)?;
            }

            if !text.contains(SMS_SUCCESS_TEXT) {
                self.save_number(&item, false);
                return Ok((
                    TaskResult::Error(InvalidPhoneNumberError.get_message()),
                    None,
                ));
            }

            self.save_number(&item, true);
            let document = Html::parse_document(&text);
            let selector = Selector::parse("form[id^='root_']")?;
            // check the first element only. the rest will be likely there if the first one is there
            let action = match document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("form[id^='root_']".to_string()))
            {
                Ok(it) => match it.value().attr("action") {
                    Some(it) => it.to_string(),
                    None => {
                        if _tries > tries {
                            continue;
                        }
                        return Ok((
                            TaskResult::Error(
                                ElementNotFoundError("action".to_string()).get_message(),
                            ),
                            None,
                        ));
                    }
                },
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);
                    if _tries > tries {
                        continue;
                    }
                    return Ok((TaskResult::Error(msg), None));
                }
            };
            let action = (BASE_URL, action.as_str()).as_url()?.to_string();

            if action.len() < 200 {
                return Err(InvalidFormActionError(action).into());
            }

            let selector = Selector::parse("input[name=fb_dtsg]")?;
            let fb_dtsg = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=fb_dtsg]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("input[name=jazoest]")?;
            let jazoest = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=jazoest]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let payload = json!({
                "fb_dtsg": fb_dtsg,
                "jazoest": jazoest,
                "code": "",
                "action_set_contact_point": "Update contact info"
            });

            return Ok((TaskResult::Success, Some((action, payload))));
        }

        Ok((TaskResult::Error(MaxTriesExceededError.get_message()), None))
    }

    fn stp6_send_invitation(
        &self,
        pc: &Consumer<String>,
        client: &Client,
        item: &String,
        url: &String,
        payload: Value,
    ) -> Result<TaskResult> {
        let _tries = RETRY_10;
        let mut tries = 0;
        let mut pool: Vec<String> = Vec::with_capacity(INVITATIONS);

        while pool.len() < INVITATIONS && _tries > tries {
            let item = match pc.dequeue() {
                Some(it) => {
                    tries = 0;
                    it
                }
                None => {
                    tries += 1;

                    if _tries < tries {
                        break;
                    }

                    thread::sleep(*SLEEP_QUEUE);
                    continue;
                }
            };

            pool.push(item);
        }

        if pool.is_empty() {
            info!("No more numbers to invite for item {}", &item);
            return Ok(TaskResult::Success);
        }

        for item in &pool {
            match pc.enqueue(item.clone()) {
                Ok(_) => {}
                Err(e) => {
                    return Ok(TaskResult::Error(e.get_message()));
                }
            }
        }

        let _tries = RETRY_3;
        tries = 0;
        let mut index = 0;
        let mut url = url.clone();
        let mut payload = payload;

        while index < pool.len() {
            let number = pool[index].clone();
            info!("Inviting {}", &number);

            let response = match client
                .post(url.clone())
                .header(header::REFERER, HeaderValue::from_str(&url)?)
                .form(&payload)
                .send()
            {
                Ok(it) => {
                    if it.status().is_success() {
                        it
                    } else {
                        warn!("{} -> Invalid response {}", &number, &it.status());
                        tries += 1;
                        if _tries > tries {
                            continue;
                        }
                        tries = 0;
                        index += 1;
                        continue;
                    }
                }
                Err(e) => {
                    error!("{} -> {}", &number, &e.get_message());
                    tries += 1;
                    if _tries > tries {
                        continue;
                    }
                    tries = 0;
                    index += 1;
                    continue;
                }
            };

            let text = match response.text() {
                Ok(it) => it,
                Err(e) => {
                    error!("{} -> {}", &number, &e.get_message());
                    tries += 1;
                    if _tries > tries {
                        continue;
                    }
                    tries = 0;
                    index += 1;
                    continue;
                }
            };

            if text.is_empty() {
                tries += 1;
                if _tries > tries {
                    continue;
                }
                tries = 0;
                index += 1;
                continue;
            }

            if self.save_responses() {
                self.save_request(6, Some(index + 1), &text)?;
            }

            if !text.contains(SMS_SUCCESS_TEXT) {
                self.save_number(&number, false);
                tries += 1;
                if _tries > tries {
                    continue;
                }
                tries = 0;
                index += 1;
                continue;
            }

            self.save_number(&number, true);
            tries = 0;
            index += 1;
            let document = Html::parse_document(&text);
            let selector = Selector::parse("form[id^='root_']")?;
            // check the first element only. the rest will be likely there if the first one is there
            let action = match document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("form[id^='root_']".to_string()))
            {
                Ok(it) => match it.value().attr("action") {
                    Some(it) => it.to_string(),
                    None => continue,
                },
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &number, &msg);
                    continue;
                }
            };
            url = (BASE_URL, action.as_str()).as_url()?.to_string();

            if url.len() < 200 {
                return Err(InvalidFormActionError(action).into());
            }

            let selector = Selector::parse("input[name=fb_dtsg]")?;
            let fb_dtsg = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=fb_dtsg]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            let selector = Selector::parse("input[name=jazoest]")?;
            let jazoest = document
                .select(&selector)
                .next()
                .ok_or_else(|| ElementNotFoundError("input[name=jazoest]".to_string()))
                .unwrap()
                .value()
                .attr("value")
                .unwrap()
                .to_string();
            payload = json!({
                "fb_dtsg": fb_dtsg,
                "jazoest": jazoest,
                "code": "",
                "action_set_contact_point": "Update contact info"
            });
        }

        Ok(TaskResult::Success)
    }

    fn save_request(&self, step: usize, minor: Option<usize>, text: &str) -> Result<()> {
        let minor = match minor {
            Some(it) => format!("_{}", it),
            None => "".to_string(),
        };

        let file_path = OUTDIR.join(format!("{:02}{}.html", &step, minor));
        info!(
            "Step {}: response is written to '{}'",
            &step,
            &file_path.display()
        );
        let file = create_with(file_path, FileOpenOptions::Truncate)?;
        write!(&file, "{}", &text)?;
        Ok(())
    }

    fn save_number(&self, number: &str, good: bool) {
        if good {
            info!(target: NUMBERS_GOOD, "{}", number);
        } else {
            info!(target: NUMBERS_BAD, "{}", number);
        }
    }

    fn save_responses(&self) -> bool {
        self.save_responses.load(Ordering::SeqCst)
    }

    fn rotate_vpn(&self, client: &Client) {
        let _tries = RETRY_10;
        let mut tries = 0;
        let vpn = match self.vpn.as_ref() {
            Some(it) => it,
            None => return,
        };
        vpn.refresh().unwrap();
        let mut locations = self.locations.lock().unwrap();
        let mut locations_used = self.locations_used.lock().unwrap();
        let mut rng = thread_rng();

        if locations.is_empty() {
            locations.append(&mut locations_used.clone());
        }

        while _tries > tries {
            if tries < 1 {
                info!("Rotating VPN location");
            } else {
                info!("Rotating VPN location [{}]", &tries);
            }

            let index = if self.vpn_random {
                rng.gen_range(0..locations.len())
            } else {
                0
            };
            let location = locations.swap_remove(index);
            info!("Connecting to location '{}'", &location);
            match vpn.connect_target(&location) {
                Ok(_) => {
                    info!("Connected to location '{}'", &location);
                    match print_ip(client) {
                        Ok(_) => {
                            locations_used.push(location.clone());
                            info!("location '{}' is good", &location);
                            break;
                        }
                        Err(e) => {
                            warn!("location '{}' is removed. {}", &location, e.get_message());
                        }
                    }
                }
                Err(e) => {
                    warn!("location '{}' is removed. {}", &location, e.get_message());
                }
            }
            tries += 1;
        }

        if tries >= _tries {
            error!("{}", MaxTriesExceededError.get_message());
        }
    }

    fn rotate_clients(&self) {
        info!("Rotating clients");
        let mut clients = self.clients.write().unwrap();
        let mut proxies = self.proxies.lock().unwrap();
        let mut proxies_used = self.proxies_used.lock().unwrap();
        let threads = self.threads.load(Ordering::SeqCst);
        clients.clear();

        if proxies.is_empty() && !proxies_used.is_empty() {
            proxies.append(&mut proxies_used.clone());
        }

        if proxies.is_empty() {
            for _ in 0..threads {
                let cookies = Arc::new(CookieStoreRwLock::new(CookieStore::default()));
                let client = match self.build_compatible_client(&cookies, None) {
                    Ok(it) => it,
                    Err(e) => {
                        panic!("Error building client: {}", e.get_message());
                    }
                };
                clients.push((Arc::new(client), cookies));
            }

            info!("Using {} new rotated clients with no proxies", threads);
            return;
        }

        let mut rng = thread_rng();

        for _ in 0..cmp::min(threads, proxies.len()) {
            let index = if self.proxy_random {
                rng.gen_range(0..proxies.len())
            } else {
                0
            };
            let proxy = proxies.swap_remove(index);
            let cookies = Arc::new(CookieStoreRwLock::new(CookieStore::default()));
            let client = match self.build_compatible_client(&cookies, Some(proxy.clone())) {
                Ok(it) => it,
                Err(e) => {
                    panic!("Error building client: {}", e.get_message());
                }
            };
            clients.push((Arc::new(client), cookies));
            proxies_used.push(proxy.clone());
        }

        info!("Using {} new rotated clients with proxies", clients.len());
    }

    fn get_clients(&self) -> Vec<(Arc<Client>, Arc<CookieStoreRwLock>)> {
        let mut clients = self.clients.read().unwrap().clone();

        if clients.is_empty() {
            drop(clients);
            self.rotate_clients();
            clients = self.clients.read().unwrap().clone();

            if self.vpn.is_some() {
                let vpn_client = clients.get(0).unwrap().0.clone();
                self.rotate_vpn(&vpn_client);
            }

            return clients;
        }

        if self.vpn_rotation == 0 && self.proxies_rotation == 0 {
            return clients;
        }

        let mut timer = self.timer.lock().unwrap();
        let elapsed = timer.unwrap().elapsed().as_secs();

        if self.proxies_rotation > 0 && elapsed > self.proxies_rotation {
            drop(clients);
            self.rotate_clients();
            clients = self.clients.read().unwrap().clone();
            *timer = Some(Instant::now());
        }

        if self.vpn.is_some() && self.vpn_rotation > 0 && elapsed > self.vpn_rotation {
            let vpn_client = clients.get(0).unwrap().0.clone();
            self.rotate_vpn(&vpn_client);
            *timer = Some(Instant::now());
        }

        clients
    }

    fn get_client(&self) -> (Arc<Client>, Arc<CookieStoreRwLock>) {
        let clients = self.get_clients();

        if clients.len() == 1 {
            clients.get(0).unwrap().clone()
        } else {
            let mut rng = thread_rng();
            clients.choose(&mut rng).unwrap().clone()
        }
    }

    fn print_step(&self, step: usize, tries: &usize, item: &String) {
        let enqueued_times = self.repeat.get(item.as_str()).map_or(1, |e| *e.value());

        if *tries < 2 {
            if enqueued_times > 1 {
                info!("Starting step {} [{}] for {}", step, enqueued_times, item);
            } else {
                info!("Starting step {} for {}", step, item);
            }

            return;
        }

        if enqueued_times > 1 {
            info!(
                "Retrying step {} [{}] [{}] for {}",
                step, enqueued_times, tries, item
            );
        } else {
            info!("Retrying step {} [{}] for {}", step, tries, item);
        }
    }

    fn print_downloading(&self, download_type: &String, tries: &usize, item: &String) {
        if download_type.is_empty() {
            return;
        }

        if *tries < 2 {
            info!("Downloading {} for {}", download_type, item);
            return;
        }

        info!("Downloading {} [{}] for {}", download_type, tries, item);
    }

    fn print_resolving_captcha(&self, file_name: &String, tries: &usize, item: &String) {
        if *tries < 2 {
            info!("Resolving Captcha file '{}' for {}", file_name, item);
            return;
        }

        info!(
            "Resolving Captcha file '{}' [{}] for {}",
            file_name, tries, item
        );
    }

    fn user_agent(&self) -> String {
        let bad_ua = self.bad_ua.read().unwrap();
        let mut user_agent = random_ua();

        while bad_ua.contains(&user_agent.to_lowercase()) {
            user_agent = random_ua();
        }

        return user_agent;

        fn random_ua() -> String {
            match random::numeric(0..2) {
                0 => random::internet::user_agent().safari().to_string(),
                1 => random::internet::user_agent().firefox().to_string(),
                _ => random::internet::user_agent().chrome().to_string(),
            }
        }
    }

    fn add_bad_ua(&self, user_agent: &str) -> bool {
        let mut bad_ua = self.bad_ua.write().unwrap();
        let user_agent = user_agent.to_lowercase();
        bad_ua.insert(user_agent.to_lowercase())
    }

    fn build_compatible_client(
        &self,
        cookies: &Arc<CookieStoreRwLock>,
        proxy: Option<String>,
    ) -> Result<Client> {
        let url = (BASE_URL, "reg/?cid=103&refsrc=deprecated&_rdr").as_url()?;
        let _tries = RETRY_100;
        let mut tries = 0;
        let proxy = match proxy {
            Some(it) => it,
            None => String::new(),
        };
        let mut user_agent = self.user_agent();

        while _tries > tries {
            tries += 1;
            let proxy_msg = if proxy.is_empty() {
                "".to_string()
            } else {
                format!(" and proxy '{}'", &proxy)
            };

            if tries < 2 {
                info!(
                    "Building compatible client with user agent '{}'{}",
                    &user_agent, &proxy_msg
                )
            } else {
                info!(
                    "Building compatible client [{}] with user agent '{}'{}",
                    &tries, &user_agent, &proxy_msg
                )
            }

            cookies.write().unwrap().clear();
            let client = if proxy.is_empty() {
                self.build_client(&user_agent)
                    .cookie_provider(cookies.clone())
                    .build()?
            } else {
                self.build_client_with_proxy(&proxy, &user_agent)
                    .cookie_provider(cookies.clone())
                    .build()?
            };

            let response = match client.post(url.clone()).send() {
                Ok(it) => {
                    if it.status().is_success() {
                        it
                    } else {
                        let err = it.error_for_status().unwrap_err();
                        error!("{} -> {}", &BASE_URL, err.get_message());
                        if _tries > tries {
                            continue;
                        }
                        return Err(err.into());
                    }
                }
                Err(e) => {
                    error!("{} -> {}", &BASE_URL, e.get_message());
                    if _tries > tries {
                        continue;
                    }
                    return Err(e.into());
                }
            };

            if !response.status().is_success() {
                let err = response.error_for_status().unwrap_err();
                error!("{} -> {}", &BASE_URL, &err.get_message());
                if _tries > tries {
                    continue;
                }
                return Err(err.into());
            }

            let text = match response.text() {
                Ok(it) => it,
                Err(e) => {
                    error!("{} -> {}", &BASE_URL, e.get_message());
                    if _tries > tries {
                        continue;
                    }
                    return Err(e.into());
                }
            };

            if text.is_empty() {
                if _tries > tries {
                    warn!("{} -> No content", &BASE_URL);
                    continue;
                }
                return Err(NoContentError.into());
            }

            if self.save_responses() {
                self.save_request(0, None, &text)?;
            }

            if text.contains("Unsupported browser") {
                let err = UnsupportedBrowserError(user_agent.clone());

                if _tries > tries {
                    warn!("{} -> {}", &BASE_URL, err.get_message());
                    self.add_bad_ua(&user_agent);
                    info!(target: UA_BAD, "{}", &user_agent);
                    user_agent = self.user_agent();
                    continue;
                }

                return Err(err.into());
            }

            return Ok(client);
        }

        Err(MaxTriesExceededError.into())
    }

    fn build_client(&self, user_agent: &str) -> ClientBuilder {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_str(&user_agent).unwrap(),
        );
        headers.insert(header::ORIGIN, HeaderValue::from_static(BASE_URL));
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"),
        );
        headers.insert(
            header::ACCEPT_LANGUAGE,
            HeaderValue::from_static("en-US,en;q=0.5"),
        );
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("gzip, deflate"),
        );
        headers.insert(
            header::UPGRADE_INSECURE_REQUESTS,
            HeaderValue::from_static("1"),
        );
        headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
        headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
        headers.insert("Sec-Fetch-Site", HeaderValue::from_static("none"));
        headers.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
        headers.insert("Te", HeaderValue::from_static("trailers"));
        let mut builder = build_blocking_client_with_user_agent(user_agent.to_owned())
            .default_headers(headers)
            .redirect(redirect::Policy::limited(u8::MAX as usize))
            .timeout(Duration::from_secs(TIMEOUT));

        if is_debug() {
            if let Some(cert) = self.load_certificate().unwrap() {
                builder = builder
                    .danger_accept_invalid_certs(true)
                    .add_root_certificate(cert);
            }
        }

        builder
    }

    fn build_client_with_proxy(&self, proxy: &str, user_agent: &str) -> ClientBuilder {
        let proxy = Proxy::https(proxy).unwrap();
        self.build_client(user_agent).proxy(proxy)
    }

    fn load_certificate(&self) -> Result<Option<Certificate>> {
        let capath = CAPATH.lock().unwrap();

        if let Some(capath) = capath.as_ref() {
            let buffer = fs::read(capath.as_str())?;
            let cert = Certificate::from_pem(&buffer)?;
            return Ok(Some(cert));
        }

        Ok(None)
    }

    fn download_captcha_audio(
        &self,
        client: &Client,
        url: &String,
        item: &String,
    ) -> Result<String> {
        let _tries = RETRY_3;
        let mut tries = 0;

        while _tries > tries {
            tries += 1;
            self.print_downloading(&"captcha audio".to_string(), &tries, item);
            let response = match client
                .get(url.clone())
                .header(header::ACCEPT, HeaderValue::from_str("*/*")?)
                .send()
            {
                Ok(it) => it,
                Err(e) => {
                    error!("{} -> {}", &item, e.get_message());
                    if _tries > tries {
                        continue;
                    }
                    return Err(e.into());
                }
            };

            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .ok_or("No Content-Type header")
                .unwrap()
                .to_str()?;
            let mime: Mime = content_type.parse()?;
            let extension = self.guess_extension_from_mime(&mime);
            let file_path = TMPDIR.join(format!(
                "{}.{}",
                random::alphanum_str(random::numeric(8..32)),
                extension
            ));
            let mut file = create_with(&file_path, FileOpenOptions::Truncate)?;
            let buffer = response.bytes()?;
            debug!("Captcha audio buffer size {} byte(s)", &buffer.len());
            file.write_all(&buffer)?;
            thread::sleep(Duration::ZERO);
            info!("Captcha audio saved to '{}'", &file_path.display());
            return Ok(file_path.to_string_lossy().to_string());
        }

        Err(DownloadError.into())
    }

    fn resolve_captcha(&self, item: &String, path: &str) -> Result<String> {
        let file_name = path::name(&path);
        let _tries = RETRY_3;
        let mut tries = 0;

        while _tries > tries {
            tries += 1;
            self.print_resolving_captcha(&file_name, &tries, &item);
            let text = match self.audio.transcribe_file(&path) {
                Ok(it) => it.replace(" ", ""),
                Err(e) => {
                    let msg = e.get_message();
                    error!("{} -> {}", &item, &msg);
                    if _tries > tries {
                        continue;
                    }
                    return Err(e);
                }
            };

            if text.is_empty() || text.len() != CAPTCHA_LEN {
                continue;
            }

            info!("Captcha resolved: {} for {}", &text, &item);
            return Ok(text);
        }

        Err(UnresolvedCaptchaAudioError.into())
    }

    fn guess_extension_from_mime(&self, mime: &Mime) -> String {
        if mime.type_() != mime::AUDIO {
            return "bin".to_string();
        }

        match mime.subtype().to_string().to_lowercase().as_str() {
            "basic" => "au".to_string(),
            "l24" => "l24".to_string(),
            "mp4" => "mp4".to_string(),
            "mpeg" => "mp3".to_string(),
            "ogg" => "ogg".to_string(),
            "vorbis" => "ogg".to_string(),
            "opus" => "opus".to_string(),
            "webm" => "webm".to_string(),
            "x-aac" => "aac".to_string(),
            "x-aiff" => "aiff".to_string(),
            "x-caf" => "caf".to_string(),
            "x-flac" => "flac".to_string(),
            "x-matroska" => "matroska".to_string(),
            "x-ms-wma" => "wma".to_string(),
            "x-ms-wav" => "wav".to_string(),
            _ => "dat".to_string(),
        }
    }
}

impl TaskDelegation<Consumer<String>, String> for TaskHandler {
    fn on_started(&self, pc: &Consumer<String>) {
        info!("Processing tasks");

        let mut started = self.started.lock().unwrap();
        *started = Some(Instant::now());

        if self.proxies_rotation > 0 {
            let mut timer = self.timer.lock().unwrap();
            *timer = Some(Instant::now());
        }

        let consumers = pc.consumers();
        debug!("Using {} consumer(s)", consumers);
    }

    fn process(&self, pc: &Consumer<String>, item: &String) -> Result<TaskResult> {
        let (client, cookies) = self.get_client();
        cookies.write().unwrap().clear();

        if self.save_responses() {
            directory::delete(&*OUTDIR).unwrap();
            debug!(
                "Responses will be written to directory '{}'",
                OUTDIR.display()
            );
        }

        let (_, payload) = match self.stp1_get_payload(&pc, &client, &item) {
            Ok((result, payload)) => match result {
                TaskResult::Success => {
                    if let Some(payload) = payload {
                        (TaskResult::Success, payload)
                    } else {
                        return Ok(TaskResult::Error(NoPayloadError.get_message()));
                    }
                }
                _ => return Ok(result),
            },
            Err(e) => {
                return Ok(TaskResult::Error(e.get_message()));
            }
        };

        let (_, form_url, payload) =
            match self.stp2_post_payload(&pc, &client, &cookies, &item, payload) {
                Ok((result, opt)) => match result {
                    TaskResult::Success => {
                        if let Some((url, payload)) = opt {
                            (TaskResult::Success, url, payload)
                        } else {
                            return Ok(TaskResult::Error(NoPayloadError.get_message()));
                        }
                    }
                    _ => return Ok(result),
                },
                Err(e) => {
                    return Ok(TaskResult::Error(e.get_message()));
                }
            };

        let (_, form_url, payload) =
            match self.stp3_post_form_action(&pc, &client, &item, &form_url, payload) {
                Ok((result, opt)) => match result {
                    TaskResult::Success => {
                        if let Some((url, payload)) = opt {
                            (TaskResult::Success, url, payload)
                        } else {
                            return Ok(TaskResult::Error(NoPayloadError.get_message()));
                        }
                    }
                    _ => return Ok(result),
                },
                Err(e) => {
                    return Ok(TaskResult::Error(e.get_message()));
                }
            };

        let (_, form_url, payload) =
            match self.stp4_post_captcha_response(&pc, &client, &item, &form_url, payload) {
                Ok((result, opt)) => match result {
                    TaskResult::Success => {
                        if let Some((url, payload)) = opt {
                            (TaskResult::Success, url, payload)
                        } else {
                            return Ok(TaskResult::Error(NoPayloadError.get_message()));
                        }
                    }
                    _ => return Ok(result),
                },
                Err(e) => {
                    return Ok(TaskResult::Error(e.get_message()));
                }
            };

        let (_, form_url, payload) =
            match self.stp5_add_mobile_number(&pc, &client, &item, &form_url, payload) {
                Ok((result, opt)) => match result {
                    TaskResult::Success => {
                        if let Some((url, payload)) = opt {
                            (TaskResult::Success, url, payload)
                        } else {
                            return Ok(TaskResult::Error(NoPayloadError.get_message()));
                        }
                    }
                    _ => return Ok(result),
                },
                Err(e) => {
                    return Ok(TaskResult::Error(e.get_message()));
                }
            };

        let result = match self.stp6_send_invitation(&pc, &client, &item, &form_url, payload) {
            Ok(it) => it,
            Err(e) => {
                return Ok(TaskResult::Error(e.get_message()));
            }
        };

        self.repeat.remove(item.as_str());
        return Ok(result);
    }

    fn on_completed(&self, pc: &Consumer<String>, item: &String, result: &TaskResult) -> bool {
        match result {
            TaskResult::Success => info!("{} -> Ok", item),
            TaskResult::Error(msg) => error!("{} -> Error: {}", item, msg),
            TaskResult::TimedOut => warn!("{} -> Timedout", item),
            _ => {}
        }

        if pc.len() == 0 && pc.running() == 1 {
            pc.complete();
        }

        true
    }

    fn on_cancelled(&self, _pc: &Consumer<String>) {
        println!("Processing tasks was cancelled");
    }

    fn on_finished(&self, _pc: &Consumer<String>) {
        if let Some(vpn) = self.vpn.as_ref() {
            info!("Disconnecting from VPN");
            vpn.disconnect().unwrap();
        }

        let started = self.started.lock().unwrap();
        let elapsed = started.unwrap().elapsed();
        info!(
            "Finished processing tasks. Took {}",
            format_duration(elapsed)
        );
    }
}
