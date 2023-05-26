use std::sync::Arc;

use coinbase_pro_rs::structs::{
    wsfeed::{Full, Level2, Match, Ticker},
    DateTime,
};
use futures::channel::mpsc;
use serde::Serialize;

use super::{NewTopicRegistrationState, Registrar, SenderCollection};

pub(super) trait Queryable: Sized {
    type Response: Send + Sync;
    fn create_fn(topic: String, sender: mpsc::Sender<Self::Response>) -> Registrar;
}

#[derive(Debug, Serialize, Clone)]
pub struct Heartbeat {
    pub sequence: u64,
    pub time: DateTime,
    pub last_trade_id: u64,
}

impl Queryable for Heartbeat {
    type Response = Self;
    fn create_fn(
        topic: String,
        sender: mpsc::Sender<Self>,
    ) -> Box<dyn FnOnce(&mut SenderCollection) -> NewTopicRegistrationState + Send> {
        Box::new(move |c| c.heartbeat.insert(topic, sender))
    }
}

impl Queryable for Full {
    type Response = Arc<Self>;
    fn create_fn(topic: String, sender: mpsc::Sender<Self::Response>) -> Registrar {
        Box::new(move |c| c.full.insert(topic, sender))
    }
}

impl Queryable for Match {
    type Response = Arc<Self>;

    fn create_fn(topic: String, sender: mpsc::Sender<Self::Response>) -> Registrar {
        Box::new(move |c| c.matchlist.insert(topic, sender))
    }
}

impl Queryable for Level2 {
    type Response = Arc<Self>;

    fn create_fn(topic: String, sender: mpsc::Sender<Self::Response>) -> Registrar {
        Box::new(move |c| c.level2.insert(topic, sender))
    }
}

impl Queryable for Ticker {
    type Response = Arc<Self>;

    fn create_fn(topic: String, sender: mpsc::Sender<Self::Response>) -> Registrar {
        Box::new(move |c| c.ticker.insert(topic, sender))
    }
}
