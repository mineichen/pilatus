use super::MinfacRouter;

pub trait ServiceCollectionExtensions {
    fn register_web(&mut self, topic: &'static str, creator: fn(super::Router) -> super::Router);
}

impl ServiceCollectionExtensions for minfac::ServiceCollection {
    fn register_web(&mut self, prefix: &'static str, creator: fn(crate::Router) -> crate::Router) {
        let route = creator(crate::Router::new(prefix));
        self.register_instance(MinfacRouter::new(route.axum_router));
        for checker in route.dependencies {
            (checker)(self);
        }
    }
}

#[cfg(test)]
mod tests {
    use minfac::{Registered, ServiceCollection};

    use super::*;
    use crate::extract::{Inject, InjectRegistered};

    async fn handler<T>(_: T) -> &'static str {
        "TestOutput"
    }

    #[test]
    pub fn fail_for_missing_dependency() {
        let mut collection = ServiceCollection::new();
        collection.register_web("foo", |r| {
            r.http("/", |x| x.get(handler::<Inject<Registered<i128>>>))
        });
        assert!(collection.build().is_err());
    }

    #[test]
    pub fn fail_for_missing_dependency_registered_injection() {
        let mut collection = ServiceCollection::new();
        collection.register_web("foo", |r| {
            r.http("/", |x| x.get(handler::<InjectRegistered<i128>>))
        });
        assert!(collection.build().is_err());
    }

    #[test]
    pub fn get_registered_sub_routers() {
        let mut collection = ServiceCollection::new();
        collection.register(|| 42i128);
        collection.register_web("foo", |r| {
            r.http("/bar", |x| x.get(handler::<Inject<Registered<i128>>>))
        });
        collection.register_web("foo", |r| {
            r.http("/foo", |x| x.get(handler::<Inject<Registered<i128>>>))
        });
        collection.register_web("foo", |r| {
            r.http("/foobar", |x| x.get(handler::<InjectRegistered<i128>>))
        });
        let provider = collection.build().unwrap();
        let all = provider
            .get_all::<super::MinfacRouter>()
            .map(|x| x.unchecked_extract());
        assert_eq!(3, all.count());
    }
}
