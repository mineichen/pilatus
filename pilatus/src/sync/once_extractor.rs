use std::sync::Mutex;

pub struct OnceExtractor<T>(Mutex<Option<T>>);

impl<T> From<T> for OnceExtractor<T> {
    fn from(value: T) -> Self {
        Self(Mutex::new(Some(value)))
    }
}

impl<T> OnceExtractor<T> {
    pub fn extract(&self) -> Option<T> {
        self.0.lock().unwrap().take()
    }
    pub fn extract_unchecked(&self) -> T {
        self.extract().expect("Value was extracted already")
    }
}

impl<T> Clone for OnceExtractor<T> {
    fn clone(&self) -> Self {
        let mut lock = self.0.lock().expect("Lock is never poisoned");
        Self(Mutex::new(lock.take()))
    }
}
