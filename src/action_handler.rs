use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{error, info};
use rustmix::{error::*, *};
use serde_json::{to_string_pretty, Value};

use crate::{common::*, service::*};

pub struct ActionHandler {
    service: Arc<Service>,
}

impl ActionHandler {
    pub fn new(service: Arc<Service>) -> Self {
        Self { service }
    }

    pub async fn process(&self, action: &YammerAction) -> Result<()> {
        output::print_header(&APP_INFO);
        match action {
            YammerAction::List {
                token,
                group_id,
                thread_id,
                email,
                all,
            } => {
                let user_id = if let Some(email) = email {
                    match self.service.get_user_id(&token, email).await {
                        Ok(it) => Some(it),
                        Err(e) => {
                            error!("{}", e.get_message());
                            return Ok(());
                        }
                    }
                } else {
                    None
                };
                let groups = if let Some(user_id) = user_id {
                    self.service.get_groups(&token, user_id).await?
                } else {
                    HashMap::new()
                };
                let count = self
                    .list(&token, *group_id, *thread_id, user_id, &groups, *all)
                    .await?;
                info!("Listed {} messages", count);
                return Ok(());
            }
            YammerAction::Delete {
                token,
                group_id,
                thread_id,
                email,
                exclude,
            } => {
                let user_id = if let Some(email) = &*email {
                    match self.service.get_user_id(&token, email).await {
                        Ok(it) => Some(it),
                        Err(e) => {
                            error!("{}", e.get_message());
                            return Ok(());
                        }
                    }
                } else {
                    None
                };
                let exclude: HashSet<u64> = exclude.iter().map(|id| id.parse().unwrap()).collect();
                let count = self
                    .service
                    .delete(&token, *group_id, *thread_id, user_id, &exclude)
                    .await?;
                info!("Deleted {} messages", count);
                return Ok(());
            }
        }
    }

    async fn list(
        &self,
        token: &str,
        group_id: Option<u64>,
        thread_id: Option<u64>,
        user_id: Option<u64>,
        groups: &HashMap<u64, String>,
        all: bool,
    ) -> Result<u64> {
        if let Some(thread_id) = thread_id {
            return self.list_thread(token, thread_id, user_id, &groups).await;
        }

        let mut messages = Vec::new();
        let mut has_more = true;
        let mut last_message_id = None;
        let mut count = 0u64;

        if let Some(group_id) = group_id {
            info!("Fetching messages for group '{}'", group_id);
        } else {
            info!("Fetching messages");
        }

        while has_more {
            has_more = self
                .service
                .get_messages(
                    &mut messages,
                    token,
                    group_id.clone(),
                    user_id.clone(),
                    last_message_id,
                )
                .await?;

            // using remove to print the messages in order (newest to oldest)
            while !messages.is_empty() {
                let message = messages.remove(0);
                last_message_id = message["id"].as_u64();

                if all {
                    let thread_id = message["thread_id"].as_u64().unwrap();
                    count += self.list_thread(token, thread_id, user_id, &groups).await?;
                } else {
                    let message = self.to_yammer_message(&message, &groups);
                    self.print_message(&message);
                    count += 1;
                }
            }
        }

        return Ok(count);
    }

    async fn list_thread(
        &self,
        token: &str,
        thread_id: u64,
        user_id: Option<u64>,
        groups: &HashMap<u64, String>,
    ) -> Result<u64> {
        let mut messages = Vec::new();
        info!("Fetching messages for thread {}", thread_id);

        if !self
            .service
            .get_messages_in_thread(&mut messages, token, thread_id, user_id)
            .await?
        {
            return Ok(0);
        }

        let count = messages.len() as u64;
        println!("Messages for thread {}", thread_id);
        let mut roots: HashMap<u64, YammerMessage> = HashMap::new();

        // using pop to print the messages in reverse order (oldest to newest)
        while let Some(message) = messages.pop() {
            let message = self.to_yammer_message(&message, groups);
            let replied_to_id = message.replied_to_id.unwrap_or(thread_id);

            if let Some(root) = roots.get_mut(&replied_to_id) {
                if root.replies.is_none() {
                    root.replies = Some(Vec::new());
                }
                root.replies.as_mut().unwrap().push(message.clone());
            } else if let Some(root) = self.find_root(&mut roots, replied_to_id) {
                if root.replies.is_none() {
                    root.replies = Some(Vec::new());
                }

                root.replies.as_mut().unwrap().push(message.clone());
            } else {
                roots.insert(replied_to_id, message.clone());
            }
        }

        for message in roots.values() {
            self.print_message(message)
        }

        return Ok(count);
    }

    fn find_root<'a>(
        &self,
        roots: &'a mut HashMap<u64, YammerMessage>,
        replied_to_id: u64,
    ) -> Option<&'a mut YammerMessage> {
        if roots.len() == 0 {
            return None;
        }

        let mut queue: Vec<&mut YammerMessage> = Vec::new();

        // Put all roots' replies arrays in the queue
        for root in roots.values_mut() {
            if let Some(replies) = &mut root.replies {
                queue.extend(replies.iter_mut());
            }
        }

        // Create a while loop to pop each entry in the queue
        while let Some(root) = queue.pop() {
            // Look for a message with an id equals to the replied_to_id
            if root.id == replied_to_id {
                return Some(root);
            }

            // If the popped message's replies is not None, add the popped message's replies to the queue
            if let Some(replies) = &mut root.replies {
                queue.extend(replies.iter_mut());
            }
        }

        None
    }

    fn to_yammer_message(&self, message: &Value, groups: &HashMap<u64, String>) -> YammerMessage {
        let group_id = message["group_id"].as_u64().unwrap_or(0);
        let group_name_def = group_id.to_string();
        let group_name = groups
            .get(&group_id)
            .map(|name| name.as_str())
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

    // fn print_json(&self, message: &Value) {
    //     let json = to_string_pretty(&message).unwrap();
    //     println!("{}", json);
    // }

    fn print_message(&self, message: &YammerMessage) {
        let json = to_string_pretty(&message).unwrap();
        println!("{}", json);
    }
}
