use core::borrow::Borrow;

use alloc::collections::BTreeMap;

pub trait SplitOffLowerThan<K, V> {
    fn split_off_lower_than<Q: ?Sized + Ord>(&mut self, key: &Q) -> BTreeMap<K, V>
    where
        K: Borrow<Q> + Ord;
}

impl<K, V> SplitOffLowerThan<K, V> for BTreeMap<K, V> {
    fn split_off_lower_than<Q: ?Sized + Ord>(&mut self, key: &Q) -> BTreeMap<K, V>
    where
        K: Borrow<Q> + Ord,
    {
        let upper = self.split_off(key);
        core::mem::replace(self, upper)
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use super::SplitOffLowerThan;

    #[test_case]
    fn basic_split() {
        let mut map: BTreeMap<i32, &str> = BTreeMap::new();
        map.insert(1, "one");
        map.insert(3, "three");
        map.insert(5, "five");
        map.insert(7, "seven");

        let lower = map.split_off_lower_than(&5);

        assert_eq!(lower.len(), 2);
        assert_eq!(lower.get(&1), Some(&"one"));
        assert_eq!(lower.get(&3), Some(&"three"));

        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&5), Some(&"five"));
        assert_eq!(map.get(&7), Some(&"seven"));
    }

    #[test_case]
    fn empty_map() {
        let mut map: BTreeMap<i32, i32> = BTreeMap::new();
        let lower = map.split_off_lower_than(&10);

        assert!(lower.is_empty());
        assert!(map.is_empty());
    }

    #[test_case]
    fn all_keys_below_threshold() {
        let mut map: BTreeMap<i32, &str> = BTreeMap::new();
        map.insert(1, "one");
        map.insert(2, "two");
        map.insert(3, "three");

        let lower = map.split_off_lower_than(&100);

        assert_eq!(lower.len(), 3);
        assert!(map.is_empty());
    }

    #[test_case]
    fn all_keys_above_threshold() {
        let mut map: BTreeMap<i32, &str> = BTreeMap::new();
        map.insert(10, "ten");
        map.insert(20, "twenty");
        map.insert(30, "thirty");

        let lower = map.split_off_lower_than(&5);

        assert!(lower.is_empty());
        assert_eq!(map.len(), 3);
    }

    #[test_case]
    fn key_at_exact_boundary() {
        let mut map: BTreeMap<i32, &str> = BTreeMap::new();
        map.insert(5, "five");
        map.insert(10, "ten");

        let lower = map.split_off_lower_than(&5);

        assert!(lower.is_empty());
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&5));
    }
}
