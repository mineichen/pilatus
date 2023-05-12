use futures::stream::{AbortHandle, AbortRegistration};
use hyper::StatusCode;
use minfac::ServiceCollection;
use pilatus::GenericConfig;
use pilatus_axum::{
    extract::{InjectRegistered, Path},
    AbortServiceInterface, ServiceCollectionExtensions,
};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub(super) fn register_services(c: &mut ServiceCollection) {
    use minfac::Registered;

    c.with::<Registered<GenericConfig>>().register_shared(|c| {
        let config = c.get::<HttpAbortSettings>("http_abort").unwrap_or_default();
        Arc::new(AbortService::new(config.limit))
    });
    c.with::<Registered<Arc<AbortService>>>()
        .register(|c| AbortServiceInterface(Box::new(move |id| c.add(id))));

    #[rustfmt::skip]
    c.register_web("abort", |x| x
        .http("/:id", |m| m.delete(abort))
    );
}

async fn abort(
    Path(id): Path<Uuid>,
    InjectRegistered(x): InjectRegistered<Arc<AbortService>>,
) -> StatusCode {
    match x.abort(id) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct HttpAbortSettings {
    limit: NonZeroUsize,
}

impl Default for HttpAbortSettings {
    fn default() -> Self {
        Self {
            limit: 30.try_into().unwrap(),
        }
    }
}

struct AbortService {
    size: NonZeroUsize,
    state: Mutex<(VecDeque<Uuid>, HashMap<Uuid, AbortHandle>)>,
}
struct NotFound;

impl AbortService {
    fn new(size: NonZeroUsize) -> Self {
        Self {
            size,
            state: Mutex::new((
                VecDeque::with_capacity(size.get()),
                HashMap::with_capacity(size.get()),
            )),
        }
    }
    // fn get(&self, item: Uuid) -> Option<AbortRegistration> {}
    fn add(&self, item: Uuid) -> Option<AbortRegistration> {
        let mut locked = self.state.lock().expect("Never poisoned");
        let (ref mut queue, ref mut unique) = locked.deref_mut();

        let is_full = queue.len() == self.size.get();
        if item.is_nil() || unique.contains_key(&item) && (!is_full || queue.front() != Some(&item))
        {
            None
        } else {
            let (token, registry) = AbortHandle::new_pair();
            if !is_full {
                queue.push_back(item);
            } else {
                queue.rotate_right(1);
                let back = queue
                    .back_mut()
                    .expect("Must have back because of len check and size is NonZero");
                unique.remove(back);
                *back = item;
            }

            unique.insert(item, token);
            Some(registry)
        }
    }
    fn abort(&self, id: Uuid) -> Result<(), NotFound> {
        let mut locked = self.state.lock().expect("Never poisoned");
        let (_, ref mut unique) = locked.deref_mut();
        unique.get(&id).ok_or(NotFound)?.abort();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_nul_uuid() {
        let set = AbortService::new(1.try_into().unwrap());
        assert!(set.add(Uuid::nil()).is_none());
    }

    #[test]
    fn insert_succeeds() {
        let set = AbortService::new(1.try_into().unwrap());
        assert!(set.add(Uuid::new_v4()).is_some());
    }
    #[test]
    fn insert_duplicate_fails() {
        let set = AbortService::new(2.try_into().unwrap());
        let first = Uuid::new_v4();
        set.add(first);
        assert!(set.add(first).is_none());
    }

    #[test]
    fn insert_duplicate_fails_if_buffer_is_full() {
        let set = AbortService::new(3.try_into().unwrap());
        let first = Uuid::new_v4();
        assert!(set.add(first).is_some());
        assert!(set.add(Uuid::new_v4()).is_some());
        assert!(set.add(Uuid::new_v4()).is_some());
        assert!(set.add(first).is_some());
    }
    #[test]
    fn insert_duplicate_if_full() {
        let set = AbortService::new(3.try_into().unwrap());
        let second = Uuid::new_v4();
        assert!(set.add(Uuid::new_v4()).is_some());
        assert!(set.add(second).is_some());
        assert!(set.add(Uuid::new_v4()).is_some());
        assert!(set.add(second).is_none());
    }
}
