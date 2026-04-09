use alloc::vec::Vec;
use core::{fmt, ops::Index};

pub struct NonEmptyVec<T> {
    first: T,
    rest: Vec<T>,
}

impl<T> NonEmptyVec<T> {
    pub fn new(first: T) -> Self {
        Self {
            first,
            rest: Vec::new(),
        }
    }

    pub fn push(mut self, item: T) -> Self {
        self.rest.push(item);
        self
    }

    pub fn len(&self) -> usize {
        1 + self.rest.len()
    }

    pub fn into_first(self) -> T {
        self.first
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        core::iter::once(&self.first).chain(self.rest.iter())
    }
}

impl<T> IntoIterator for NonEmptyVec<T> {
    type Item = T;
    type IntoIter = core::iter::Chain<core::iter::Once<T>, alloc::vec::IntoIter<T>>;

    fn into_iter(self) -> Self::IntoIter {
        core::iter::once(self.first).chain(self.rest)
    }
}

impl<T> Index<usize> for NonEmptyVec<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        if index == 0 {
            &self.first
        } else {
            &self.rest[index - 1]
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for NonEmptyVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::NonEmptyVec;

    #[test_case]
    fn new_creates_single_element() {
        let v = NonEmptyVec::new(42);
        assert!(v.len() == 1);
        assert!(v[0] == 42);
    }

    #[test_case]
    fn push_grows_and_preserves_order() {
        let v = NonEmptyVec::new(1).push(2).push(3);
        assert!(v.len() == 3);
        assert!(v[0] == 1);
        assert!(v[1] == 2);
        assert!(v[2] == 3);
    }

    #[test_case]
    fn into_first_returns_first_element() {
        let v = NonEmptyVec::new(10).push(20);
        assert!(v.into_first() == 10);
    }

    #[test_case]
    fn into_iter_yields_all_in_order() {
        let v = NonEmptyVec::new(1).push(2).push(3);
        let collected: vec::Vec<i32> = v.into_iter().collect();
        assert!(collected == vec![1, 2, 3]);
    }

    #[test_case]
    fn iter_yields_references_in_order() {
        let v = NonEmptyVec::new(10).push(20);
        let collected: vec::Vec<&i32> = v.iter().collect();
        assert!(*collected[0] == 10);
        assert!(*collected[1] == 20);
    }
}
