use std::{borrow::Borrow, collections::HashMap, hash::Hash};

#[derive(Default)]
pub struct OccuranceCounter<T>(HashMap<T, usize>);

impl<TItem: Eq + Hash> OccuranceCounter<TItem> {
    pub fn remove(&mut self, item: impl Borrow<TItem>) -> bool {
        let item = item.borrow();
        let Some(val) = self.0.get_mut(item) else {
            return false;
        };

        if *val > 1 {
            *val -= 1;
        } else {
            self.0.remove(item);
        }
        true
    }

    pub fn len(&self) -> usize {
        self.0.values().copied().sum()
    }
}

impl<TItem: Eq + Hash> FromIterator<TItem> for OccuranceCounter<TItem> {
    fn from_iter<T: IntoIterator<Item = TItem>>(iter: T) -> Self {
        let mut ctr = OccuranceCounter(HashMap::default());
        ctr.extend(iter);
        ctr
    }
}

impl<TItem: Eq + Hash> Extend<TItem> for OccuranceCounter<TItem> {
    fn extend<T: IntoIterator<Item = TItem>>(&mut self, iter: T) {
        let iter = iter.into_iter();
        self.0.reserve(iter.size_hint().0);
        for x in iter {
            *self.0.entry(x).or_insert(0) += 1;
        }
    }
}
