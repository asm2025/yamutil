use std::{collections::HashMap, sync::Arc};

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
                    .list(&token, *group_id, *thread_id, user_id, &groups)
                    .await?;
                info!("Listed {} messages", count);
                return Ok(());
            }
            YammerAction::Delete {
                token,
                group_id,
                thread_id,
                email,
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
                let count = self
                    .service
                    .delete(&token, *group_id, *thread_id, user_id)
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

            while !messages.is_empty() {
                let message = messages.remove(0);
                last_message_id = message["id"].as_u64();
                let thread_id = message["thread_id"].as_u64().unwrap();
                count += self.list_thread(token, thread_id, user_id, &groups).await?;
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

        // using pop to print the messages in reverse order (oldest to newest)
        while let Some(message) = messages.pop() {
            self.print_message(&message, &groups);
        }

        return Ok(count);
    }

    fn print_message(&self, message: &Value, groups: &HashMap<u64, String>) {
        let group_id = message["group_id"].as_u64().unwrap_or(0);
        let group_name_def = group_id.to_string();
        let group_name = groups
            .get(&group_id)
            .map(|name| name.as_str())
            .unwrap_or(&group_name_def);
        let message = SelectedMessage {
            id: message["id"].as_u64().unwrap_or(0),
            sender_id: message["sender_id"].as_u64().unwrap_or(0),
            network_id: message["network_id"].as_u64().unwrap_or(0),
            group_id: group_id,
            group_name: group_name.to_owned(),
            thread_id: message["thread_id"].as_u64().unwrap_or(0),
            privacy: message["privacy"].as_str().unwrap().to_owned(),
            created_at: message["created_at"].as_str().unwrap().to_owned(),
            body: message["body"]["rich"].as_str().unwrap().to_owned(),
            liked_by: message["liked_by"]["count"].as_u64().unwrap_or(0),
        };
        let json = to_string_pretty(&message).unwrap();
        println!("{}", json);
    }
}
