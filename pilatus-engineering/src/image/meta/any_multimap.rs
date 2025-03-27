use std::{
    any::{Any, TypeId},
    cell::{Ref, RefCell, RefMut},
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::Arc,
};

/// Passing untyped values. This type is not !Sync on purpose to allow
/// .get_mut() to only require a shared reference to &self.
///
/// On Clone, the HashMap-structure is cloned too, but all values are stored in a Arc,
/// and thus only cloned on demand if get_mut is called and another copy of this AnyMultiMap exists
#[derive(Default, Clone)]
pub struct AnyMultiMap {
    map: HashMap<TypeId, smallvec::SmallVec<[RefCell<Arc<dyn Any + Send + Sync + 'static>>; 1]>>,
}

impl Debug for AnyMultiMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.map.keys().fmt(f)
    }
}

impl AnyMultiMap {
    pub fn try_get<T: Any>(&self) -> Result<Option<Ref<'_, T>>, std::cell::BorrowError> {
        Ok(self
            .map
            .get(&std::any::TypeId::of::<T>())
            .and_then(|x| x.last())
            .map(|x| x.try_borrow())
            .transpose()?
            .and_then(|x| std::cell::Ref::filter_map(x, |y| y.downcast_ref::<T>()).ok()))
    }

    /// Panics, if this item is borrowed already, use try_get if this is not desired
    pub fn get<T: Any>(&self) -> Option<Ref<'_, T>> {
        self.map
            .get(&std::any::TypeId::of::<T>())
            .and_then(|x| x.last())
            .and_then(|x| std::cell::Ref::filter_map(x.borrow(), |y| y.downcast_ref::<T>()).ok())
    }

    pub fn try_get_mut<T: Any + Clone + Send + Sync>(
        &self,
    ) -> Result<Option<RefMut<'_, T>>, std::cell::BorrowMutError> {
        Ok(self
            .map
            .get(&std::any::TypeId::of::<T>())
            .and_then(|x| x.last())
            .map(|x| x.try_borrow_mut())
            .transpose()?
            .and_then(|x| {
                std::cell::RefMut::filter_map(x, |x| {
                    if Arc::get_mut(x).is_none() {
                        *x = Arc::new(
                            x.downcast_ref::<T>()
                                .expect("Values for Key with typeof(T) are castable")
                                .clone(),
                        );
                    }
                    return Arc::get_mut(x)
                        .expect("Created if empty")
                        .downcast_mut::<T>();
                })
                .ok()
            }))
    }

    /// Panics, if this item is borrowed already, use try_get_mut if this is not desired
    /// This method clones the value, if another cloned instance of this anymap exists
    pub fn get_mut<T: Any + Clone + Send + Sync>(&self) -> Option<RefMut<'_, T>> {
        self.map
            .get(&std::any::TypeId::of::<T>())
            .and_then(|x| x.last())
            .and_then(|x| {
                std::cell::RefMut::filter_map(x.borrow_mut(), |x| {
                    if Arc::get_mut(x).is_none() {
                        *x = Arc::new(
                            x.downcast_ref::<T>()
                                .expect("Values for Key with typeof(T) are castable")
                                .clone(),
                        );
                    }
                    return Arc::get_mut(x)
                        .expect("Created if empty")
                        .downcast_mut::<T>();
                })
                .ok()
            })
    }

    pub fn take<T: Any + Clone + Send + Sync>(&mut self) -> Option<T> {
        self.map
            .remove(&std::any::TypeId::of::<T>())
            .and_then(|mut x| x.pop())
            .and_then(|x| {
                x.into_inner()
                    .downcast::<T>()
                    .ok()
                    .map(Arc::unwrap_or_clone)
            })
    }

    pub fn insert_arced<T: Any + Send + Sync + 'static>(&mut self, item: Arc<T>) {
        let item = RefCell::new(item as Arc<dyn Any + Send + Sync + 'static>);
        match self.map.entry(std::any::TypeId::of::<T>()) {
            Entry::Occupied(mut occupied_entry) => occupied_entry.get_mut().push(item),
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(smallvec::smallvec![item]);
            }
        }
    }

    pub fn insert<T: Any + Send + Sync + 'static>(&mut self, item: T) {
        self.insert_arced(Arc::new(item));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut map = AnyMultiMap::default();
        map.insert(0i32);
        map.insert(42i32);
        assert_eq!(Some(42i32), map.get::<i32>().map(|x| *x));
    }

    #[test]
    fn take_item() {
        let mut map = AnyMultiMap::default();
        map.insert("Foo".to_string());
        assert_eq!("Foo", map.take::<String>().unwrap())
    }
    #[test]
    fn insert_and_get_mut_twice() {
        let mut map = AnyMultiMap::default();
        map.insert(0i32);
        map.insert(0u32);
        {
            let mut mutable = map.get_mut::<i32>().unwrap();
            *mutable = 42;
        }
        let a = map.get::<u32>().unwrap();

        assert_eq!(Some(42i32), map.get::<i32>().map(|x| *x));
        assert_eq!(0, *a)
    }
    #[test]
    fn insert_and_get_shared() {
        let mut map = AnyMultiMap::default();
        map.insert(0i32);
        let map2 = map.clone();
        *map.get_mut::<i32>().unwrap() = 42;

        assert_eq!(Some(42i32), map.get::<i32>().map(|x| *x));
        assert_eq!(Some(0i32), map2.get::<i32>().map(|x| *x));
    }
}
