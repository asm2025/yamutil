use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use log::{error, info};
use rustmix::{error::*, *};

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
                let count = self
                    .list(&token, *group_id, *thread_id, user_id, *all)
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
        all: bool,
    ) -> Result<u64> {
        let mut groups = HashMap::new();

        if let Some(user_id) = user_id {
            self.service.get_groups(&mut groups, token, user_id).await?;
        }

        if let Some(thread_id) = thread_id {
            return self
                .list_thread(token, thread_id, user_id, &mut groups)
                .await;
        }

        let mut messages = VecDeque::new();
        let mut has_more = true;
        let mut last_message_id = None;
        let mut count = 0u64;

        if let Some(group_id) = group_id {
            info!("Fetching messages for group '{}'", group_id);
        } else {
            info!("Fetching messages");
        }

        let uid = user_id.unwrap_or(0);

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

            // using pop_front to print the messages in order (newest/child to oldest/parent)
            while let Some(message) = messages.pop_front() {
                last_message_id = message["id"].as_u64();

                if all {
                    let thread_id = message["thread_id"].as_u64().unwrap();
                    count += self
                        .list_thread(token, thread_id, user_id, &mut groups)
                        .await?;
                } else {
                    let group_id = message["group_id"].as_u64().unwrap();

                    if !groups.contains_key(&group_id) {
                        let muid = message["sender_id"].as_u64().unwrap();

                        if uid != muid {
                            self.service.get_groups(&mut groups, token, muid).await?;
                        }
                    }

                    let message = to_yammer_message(&message, &groups);
                    print_message(&message);
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
        groups: &mut HashMap<u64, YammerGroup>,
    ) -> Result<u64> {
        info!("Fetching messages for thread {}", thread_id);
        let mut messages = VecDeque::new();
        self.service
            .get_messages_in_thread(&mut messages, token, thread_id, user_id)
            .await?;

        if messages.is_empty() {
            return Ok(0);
        }

        let count = messages.len() as u64;
        println!("Messages for thread {}", thread_id);
        let mut roots: HashMap<u64, YammerMessage> = HashMap::new();
        let uid = user_id.unwrap_or(0);

        while let Some(message) = messages.pop_back() {
            let group_id = message["group_id"].as_u64().unwrap();

            if !groups.contains_key(&group_id) {
                let muid = message["sender_id"].as_u64().unwrap();

                if uid != muid {
                    self.service.get_groups(groups, token, muid).await?;
                }
            }

            let message = to_yammer_message(&message, &groups);
            let replied_to_id = message.replied_to_id.unwrap_or(thread_id);

            if let Some(root) = roots.get_mut(&replied_to_id) {
                if root.replies.is_none() {
                    root.replies = Some(Vec::new());
                }
                root.replies.as_mut().unwrap().push(message.clone());
            } else if let Some(root) = find_root(&mut roots, replied_to_id) {
                if root.replies.is_none() {
                    root.replies = Some(Vec::new());
                }

                root.replies.as_mut().unwrap().push(message.clone());
            } else {
                roots.insert(replied_to_id, message.clone());
            }
        }

        for message in roots.values() {
            print_message(message)
        }

        return Ok(count);

        fn find_root<'a>(
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
    }
}
