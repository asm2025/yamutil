#![allow(dead_code)]
use log::{error, info, warn};
use reqwest_cookie_store::{CookieStore, CookieStoreRwLock};
use rustmix::{
    error::*,
    web::reqwest::{Client, RequestBuilder, Response},
    *,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Mutex, time::sleep};

use crate::common::*;

const BASE_URL: &str = "https://www.yammer.com/api/v1/";
const RLT_10: u64 = 10;
const RLT_30: u64 = 30;

#[derive(Debug, Clone)]
pub struct Service {
    client: Arc<Client>,
    bucket: Arc<Mutex<TokenBucket>>,
}

impl Service {
    pub fn new() -> Self {
        let cookies = Arc::new(CookieStoreRwLock::new(CookieStore::default()));
        let client = match build_compatible_client(&cookies) {
            Ok(it) => Arc::new(it),
            Err(e) => {
                panic!("Error building client: {}", e.get_message());
            }
        };
        let bucket = Arc::new(Mutex::new(TokenBucket::new(10, 30)));
        Self { client, bucket }
    }

    pub async fn get_user_id(&self, token: &str, user_email: &str) -> Result<u64> {
        if user_email.is_empty() {
            return Err(InvalidEmailError.into());
        }

        info!("Fetching user information for email '{}'", user_email);
        let url = format!(
            "{}users/by_email.json?email={}",
            BASE_URL,
            urlencoding::encode(&user_email)
        );
        let response = self
            .send_with_rate_limit(
                self.client
                    .get(&url)
                    .header("authorization", format!("Bearer {}", &token)),
                RLT_10,
            )
            .await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err().into());
        }

        let json = response.json::<Value>().await?;
        let id = json
            .as_array()
            .and_then(|items| items.iter().find(|u| u["type"] == "user"))
            .and_then(|user| user["id"].as_u64());

        if let Some(id) = id {
            info!("User id for email '{}' is found: {}", user_email, id);
            return Ok(id);
        }

        warn!("User id for email '{}' is not found", user_email);
        return Err(InvalidEmailError.into());
    }

    pub async fn get_user_info(&self, token: &str, user_id: u64) -> Result<YammerUser> {
        info!("Fetching user information for id '{}'", user_id);
        let url = format!("{}users/{}.json", BASE_URL, user_id);
        let response = self
            .send_with_rate_limit(
                self.client
                    .get(&url)
                    .header("authorization", format!("Bearer {}", &token)),
                RLT_10,
            )
            .await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err().into());
        }

        let json = response.json::<Value>().await?;
        Ok(YammerUser {
            id: json["id"].as_u64().unwrap(),
            name: json["full_name"].as_str().unwrap().to_string(),
            email: json["email"].as_str().unwrap().to_string(),
        })
    }

    pub async fn get_users<C>(
        &self,
        collection: &mut C,
        token: &str,
        page: u32,
        num_per_page: u32,
    ) -> Result<()>
    where
        C: Extend<(u64, YammerUser)> + Send,
    {
        info!(
            "Fetching users for page {} and num_per_page {}",
            page, num_per_page
        );
        let url = format!(
            "{}users.json?page={}&num_per_page={}",
            BASE_URL, page, num_per_page
        );
        let response = self
            .send_with_rate_limit(
                self.client
                    .get(&url)
                    .header("authorization", format!("Bearer {}", &token)),
                RLT_10,
            )
            .await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err().into());
        }

        let json = response.json::<Value>().await?;
        let users = json
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "user")
            .map(|e| {
                let user = YammerUser {
                    id: e["id"].as_u64().unwrap(),
                    name: e["full_name"].as_str().unwrap().to_string(),
                    email: e["email"].as_str().unwrap().to_string(),
                };
                (user.id, user)
            });
        collection.extend(users);
        Ok(())
    }

    pub async fn get_user_groups<C>(
        &self,
        collection: &mut C,
        token: &str,
        user_id: u64,
    ) -> Result<()>
    where
        C: Extend<(u64, YammerGroup)> + Send,
    {
        info!("Fetching groups for user '{}'", user_id);
        let url = format!("{}groups/for_user/{}.json", BASE_URL, &user_id);
        let response = self
            .send_with_rate_limit(
                self.client
                    .get(&url)
                    .header("authorization", format!("Bearer {}", &token)),
                RLT_10,
            )
            .await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err().into());
        }

        let json = response.json::<Value>().await?;
        let groups = json
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "group")
            .map(|e| {
                let group = YammerGroup {
                    id: e["id"].as_u64().unwrap(),
                    name: e["name"].as_str().unwrap().to_string(),
                    display_name: e["full_name"].as_str().unwrap().to_string(),
                };
                (group.id, group)
            });
        collection.extend(groups);
        Ok(())
    }

    pub async fn get_group_users<C>(
        &self,
        collection: &mut C,
        token: &str,
        group_id: u64,
        page: u32,
    ) -> Result<()>
    where
        C: Extend<(u64, YammerUser)> + Send,
    {
        info!("Fetching users in group {} for page {}", group_id, page);
        let url = format!("{}users/in_group/{}.json&page={}", BASE_URL, group_id, page);
        let response = self
            .send_with_rate_limit(
                self.client
                    .get(&url)
                    .header("authorization", format!("Bearer {}", &token)),
                RLT_10,
            )
            .await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err().into());
        }

        let json = response.json::<Value>().await?;
        let users = json
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "user")
            .map(|e| {
                let user = YammerUser {
                    id: e["id"].as_u64().unwrap(),
                    name: e["full_name"].as_str().unwrap().to_string(),
                    email: e["email"].as_str().unwrap().to_string(),
                };
                (user.id, user)
            });
        collection.extend(users);
        Ok(())
    }

    pub async fn get_messages<C>(
        &self,
        collection: &mut C,
        token: &str,
        group_id: Option<u64>,
        user_id: Option<u64>,
        last_message_id: Option<u64>,
    ) -> Result<bool>
    where
        C: Extend<Value> + Send,
    {
        info!("Fetching messages");
        let p_message = if let Some(lmid) = last_message_id {
            format!("&older_than={}", lmid)
        } else {
            String::new()
        };
        let url = if let Some(group_id) = group_id {
            format!(
                "{}messages/in_group/{}.json?threaded=true{}",
                BASE_URL, group_id, p_message
            )
        } else {
            format!("{}messages/sent.json?threaded=true{}", BASE_URL, p_message)
        };
        let response = self
            .send_with_rate_limit(
                self.client
                    .get(&url)
                    .header("authorization", format!("Bearer {}", &token)),
                RLT_10,
            )
            .await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err().into());
        }

        let feed = response.json::<Value>().await?;
        let messages = feed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| {
                e["sender_type"] == "user"
                    && (user_id.is_none() || e["sender_id"].as_u64() == user_id)
            })
            .cloned();
        collection.extend(messages);
        let older_available = feed["meta"]["older_available"].as_bool().unwrap_or(false);
        return Ok(older_available);
    }

    pub async fn get_messages_in_thread<C>(
        &self,
        collection: &mut C,
        token: &str,
        thread_id: u64,
        user_id: Option<u64>,
    ) -> Result<()>
    where
        C: Extend<Value> + Send,
    {
        info!("Fetching messages for thread {}", thread_id);
        let url = format!("{}messages/in_thread/{}.json", BASE_URL, &thread_id);
        let response = self
            .send_with_rate_limit(
                self.client
                    .get(&url)
                    .header("authorization", format!("Bearer {}", &token)),
                RLT_10,
            )
            .await?;

        if !response.status().is_success() {
            return Err(response.error_for_status().unwrap_err().into());
        }

        let feed = response.json::<Value>().await?;
        let messages = feed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| {
                e["sender_type"] == "user"
                    && (user_id.is_none() || e["sender_id"].as_u64() == user_id)
            })
            .cloned();
        collection.extend(messages);
        return Ok(());
    }

    pub async fn delete(
        &self,
        token: &str,
        group_id: Option<u64>,
        thread_id: Option<u64>,
        user_id: Option<u64>,
        exclude: &HashSet<u64>,
    ) -> Result<u64> {
        let mut groups = HashMap::new();

        if let Some(user_id) = user_id {
            self.get_user_groups(&mut groups, token, user_id).await?;
        }

        if let Some(thread_id) = thread_id {
            return self
                .delete_thread(token, thread_id, user_id, &mut groups)
                .await;
        }

        // rate limit already taken in get_messages
        let mut messages = VecDeque::new();
        let mut has_more = true;
        let mut last_message_id = None;
        let mut count = 0u64;

        if let Some(group_id) = group_id {
            info!("Fetching messages for deletion in group '{}'", group_id);
        } else {
            info!("Fetching messages for deletion");
        }

        let uid = user_id.unwrap_or(0);

        while has_more {
            has_more = self
                .get_messages(
                    &mut messages,
                    token,
                    group_id.clone(),
                    user_id.clone(),
                    last_message_id,
                )
                .await?;

            while let Some(message) = messages.pop_front() {
                last_message_id = message["id"].as_u64();
                let message_id = last_message_id.unwrap();
                let thread_id = message["thread_id"].as_u64().unwrap();

                if exclude.contains(&message_id) || self.has_likes(&message, user_id) {
                    info!(
                        "Skipping message '{}' and aborting thread '{}'",
                        message_id, thread_id
                    );
                    continue;
                }

                let group_id = message["group_id"].as_u64().unwrap();

                if !groups.contains_key(&group_id) {
                    let muid = message["sender_id"].as_u64().unwrap();

                    if uid != muid {
                        self.get_user_groups(&mut groups, token, muid).await?;
                    }
                }

                count += self
                    .delete_thread(token, thread_id, user_id, &mut groups)
                    .await?;
            }
        }

        return Ok(count);
    }

    pub async fn delete_thread(
        &self,
        token: &str,
        thread_id: u64,
        user_id: Option<u64>,
        groups: &mut HashMap<u64, YammerGroup>,
    ) -> Result<u64> {
        // rate limit already taken in get_messages_in_thread
        info!("Fetching messages for thread {} for deletion", thread_id);
        let mut messages = VecDeque::new();
        // We will get ALL messages in the thread, not just the user's messages because we have to skip threads with likes and qothers' messages
        self.get_messages_in_thread(&mut messages, token, thread_id, None)
            .await?;

        if messages.is_empty() {
            return Ok(0);
        }

        let mut count = 0u64;
        info!("Deleting messages for thread {}", thread_id);

        // using pop_front to delete the messages in order (newest/child to oldest/parent)
        while let Some(message) = messages.pop_front() {
            // We will only delete the user's messages that has no interactions
            if self.has_likes(&message, user_id)
                || (user_id.is_some() && message["sender_id"].as_u64() != user_id)
            {
                info!(
                    "Skipping message '{}' and aborting thread '{}'",
                    message["id"].as_u64().unwrap(),
                    thread_id
                );
                break;
            }

            let message = YammerMessage::from_json(&message, groups);
            let url = format!("{}messages/{}.json", BASE_URL, &message.id);
            let response = self
                .send_with_rate_limit(
                    self.client
                        .delete(&url)
                        .header("authorization", format!("Bearer {}", &token)),
                    RLT_30,
                )
                .await?;

            if !response.status().is_success() {
                error!(
                    "Error deleting message '{}': {}",
                    &message.id,
                    response.text().await?
                );
                info!(
                    "Skipping message '{}' and aborting thread '{}'",
                    &message.id, thread_id
                );
                break;
            }
            info!("Deleted message '{}'", &message.id);
            output::print_message(&message);
            count += 1;
        }

        return Ok(count);
    }

    pub fn has_likes(&self, message: &Value, user_id: Option<u64>) -> bool {
        let liked_by = message["liked_by"]["count"].as_u64().unwrap_or(0);

        if liked_by == 0 {
            return false;
        }

        if user_id.is_none() {
            return true;
        }

        let liked_by = message["liked_by"]["names"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["user_id"].as_u64() != user_id);
        return liked_by;
    }

    async fn send_with_rate_limit(
        &self,
        request: RequestBuilder,
        rate_limit: u64,
    ) -> Result<Response> {
        loop {
            let mut bkt = self.bucket.lock().await;

            if bkt.take() {
                break;
            }

            sleep(Duration::from_secs(1)).await;
        }

        let mut tries = 0;
        let response = loop {
            let req = request.try_clone().expect("Failed to clone request");
            match req.send().await {
                Ok(it) => {
                    if it.status() == 429 {
                        if tries > 3 {
                            return Err(RateLimitTimeoutExceededError.into());
                        }
                        warn!("Rate limit exceeded. Waiting for {} seconds", rate_limit);
                        sleep(Duration::from_secs(rate_limit)).await;
                        tries += 1;
                        continue;
                    } else {
                        break it;
                    }
                }
                Err(e) => return Err(e.into()),
            }
        };
        Ok(response)
    }
}
