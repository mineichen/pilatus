use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

use coinbase_pro_rs::structs::wsfeed::{Full, Level2, Match, Message, Ticker};
use futures::channel::mpsc;
use tracing::{debug, error, warn};

use super::{queryable::Queryable, HandleResult, Heartbeat, NewTopicRegistrationState};

#[derive(Default, Debug)]
pub(super) struct SenderCollection {
    pub heartbeat: SenderList<Heartbeat>,
    pub full: SenderList<Arc<Full>>,
    pub matchlist: SenderList<Arc<Match>>,
    pub level2: SenderList<Arc<Level2>>,
    pub ticker: SenderList<Arc<Ticker>>,
}

impl SenderCollection {
    pub fn get_active_topics(&self) -> Vec<&str> {
        self.heartbeat
            .0
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
    }
    fn is_empty(&self) -> bool {
        self.heartbeat.0.is_empty() && self.full.0.is_empty()
    }
    pub fn handle_message(&mut self, msg: Message) -> HandleResult {
        let handler_result = match msg {
            Message::Heartbeat {
                sequence,
                last_trade_id,
                product_id,
                time,
            } => self.heartbeat.handle(
                product_id,
                Heartbeat {
                    sequence: sequence as _,
                    time,
                    last_trade_id: last_trade_id as _,
                },
            ),
            Message::Full(x) => self.full.handle(x.product_id().to_string(), Arc::new(x)),
            Message::Level2(x) => self.level2.handle(x.product_id().to_string(), Arc::new(x)),
            Message::Ticker(x) => self.ticker.handle(x.product_id().to_string(), Arc::new(x)),
            Message::Match(x) => self.matchlist.handle(x.product_id.clone(), Arc::new(x)),

            m => {
                warn!("Unknown message {m:?} is ignored");
                Ok(())
            }
        };
        match handler_result {
            Ok(_) => HandleResult::Ok,
            Err(_) => {
                if self.is_empty() {
                    debug!("All listeners exited. Stream to coinbase closes");
                    HandleResult::StopStreaming
                } else {
                    HandleResult::RequireStreamRestart
                }
            }
        }
    }
}

pub(super) struct SenderList<T>(HashMap<String, Vec<mpsc::Sender<T>>>);

impl<T: Clone> SenderList<T> {
    pub fn insert(&mut self, topic: String, sender: mpsc::Sender<T>) -> NewTopicRegistrationState {
        match self.0.entry(topic) {
            Entry::Occupied(mut x) => {
                x.get_mut().push(sender);
                NewTopicRegistrationState::ExistedAlready
            }
            Entry::Vacant(x) => {
                x.insert(vec![sender]);
                NewTopicRegistrationState::Created
            }
        }
    }

    fn handle(&mut self, product_id: String, msg: T) -> Result<(), ()> {
        let Some(topic) = self.0.get_mut(&product_id) else {
            error!("Websocket sent broadcast for product_id noone is listening to");
            return Ok(());
        };
        let prev_len = topic.len();
        topic.retain_mut(|v| v.try_send(msg.clone()).is_ok());
        debug!(
            "Listeners on topic {product_id}, Prev: {prev_len}, now: {}",
            topic.len()
        );
        if topic.is_empty() {
            self.0.remove(&product_id).expect("Must have been there");
            Err(())
        } else {
            Ok(())
        }
    }
}

impl<T> Default for SenderList<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T> std::fmt::Debug for SenderList<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SenderList").field(&self.0.len()).finish()
    }
}
