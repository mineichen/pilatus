use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use futures::{future::BoxFuture, FutureExt};
use tokio::sync::{Mutex, MutexGuard};

/// Used to extract a Variable which contains a subset of data
/// ```
/// use pilatus::{AccessibleValue, Accessor};
/// use std::sync::Arc;
/// use tokio::sync::Mutex;
///
/// struct Outer {
///     inner: i32,
/// }
/// impl AsRef<i32> for Outer {
///     fn as_ref(&self) -> &i32 {
///         &self.inner
///     }
/// }
/// impl AsMut<i32> for Outer {
///     fn as_mut(&mut self) -> &mut i32 {
///         &mut self.inner
///     }
/// }
///
/// async fn test(x: impl Accessor<i32>) {
///     let mut lock = x.lock().await;
///     assert_eq!(&42, lock.as_ref());
///     assert_eq!(&mut 42, lock.as_mut());
/// }
/// test(Arc::new(Mutex::new(Outer { inner: 42 })));
///
/// ```
pub trait Accessor<T> {
    type Lock<'a>: AccessibleValue<'a, T>
    where
        Self: 'a;
    fn lock(&self) -> BoxFuture<'_, Self::Lock<'_>>;
}

impl<TIn: Send + 'static + AsRef<TOut> + AsMut<TOut>, TOut> Accessor<TOut>
    for tokio::sync::Mutex<TIn>
{
    type Lock<'a> = MutexGuard<'a, TIn>;

    // Change when async trait-fn exist (they don't around 18.01.2023)
    fn lock(&self) -> BoxFuture<'_, Self::Lock<'_>> {
        Mutex::lock(self).boxed()
    }
}

impl<T: Accessor<TOut> + 'static, TOut> Accessor<TOut> for Arc<T> {
    type Lock<'a> = T::Lock<'a>;

    fn lock(&self) -> BoxFuture<'_, Self::Lock<'_>> {
        (**self).lock()
    }
}

pub trait AccessibleValue<'a, T> {
    fn as_ref(&self) -> &T;
    fn as_mut(&mut self) -> &mut T;
}

impl<'a, TIn: AsRef<TOut> + AsMut<TOut>, TOut> AccessibleValue<'a, TOut> for MutexGuard<'a, TIn> {
    fn as_ref(&self) -> &TOut {
        self.deref().as_ref()
    }

    fn as_mut(&mut self) -> &mut TOut {
        self.deref_mut().as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn without_asref() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        struct Outer {
            inner: i32,
        }
        impl AsRef<Outer> for Outer {
            fn as_ref(&self) -> &Outer {
                self
            }
        }

        impl AsMut<Outer> for Outer {
            fn as_mut(&mut self) -> &mut Outer {
                self
            }
        }

        async fn test(x: impl Accessor<Outer>) {
            let mut lock = x.lock().await;
            let _outer = lock.as_ref();
            let outer_mut = lock.as_mut();
            assert_eq!(42, outer_mut.inner);
        }
        let _unused = tokio::task::spawn(test(Arc::new(Mutex::new(Outer { inner: 42 }))));
    }
}
