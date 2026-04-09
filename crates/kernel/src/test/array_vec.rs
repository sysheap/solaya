#[cfg(test)]
mod tests {
    use crate::klibc::array_vec::{ArrayVec, ArrayVecError};
    use alloc::rc::Rc;
    use core::cell::Cell;

    #[test_case]
    fn max_size() {
        let mut array_vec: ArrayVec<u64, 2> = ArrayVec::default();

        assert!(array_vec.pop().is_none());

        array_vec.push(1).expect("There should be still space");
        array_vec.push(2).expect("There should be still space");
        assert_eq!(array_vec.push(3), Err(ArrayVecError::NoSpaceLeft(3)));

        assert_eq!(array_vec.pop(), Some(2));
        assert_eq!(array_vec.pop(), Some(1));
        assert_eq!(array_vec.pop(), None);
    }

    #[test_case]
    fn drop_check() {
        let counter = Rc::new(Cell::new(0));

        struct DropElement {
            counter: Rc<Cell<u64>>,
        }

        impl Drop for DropElement {
            fn drop(&mut self) {
                self.counter.set(self.counter.get() + 1);
            }
        }

        let mut array_vec: ArrayVec<DropElement, 2> = ArrayVec::default();

        let _ = array_vec.push(DropElement {
            counter: counter.clone(),
        });
        let _ = array_vec.push(DropElement {
            counter: counter.clone(),
        });

        assert_eq!(counter.get(), 0);
        drop(array_vec);
        assert_eq!(counter.get(), 2);
    }

    #[test_case]
    fn iter() {
        let mut array_vec: ArrayVec<u64, 3> = ArrayVec::default();

        let _ = array_vec.push(1);
        let _ = array_vec.push(2);

        let mut iter = array_vec.iter();

        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), None);
    }

    #[test_case]
    fn as_slice() {
        let mut array_vec: ArrayVec<u64, 3> = ArrayVec::default();

        let _ = array_vec.push(1);
        let _ = array_vec.push(2);

        assert_eq!(array_vec.last(), Some(&2));

        let _ = array_vec.push(3);
        assert_eq!(array_vec.last(), Some(&3));

        let last = array_vec.last_mut().expect("There must be a last element");

        *last = 42;
        assert_eq!(array_vec.last(), Some(&42));

        assert_eq!(array_vec.pop(), Some(42));
        assert_eq!(array_vec.pop(), Some(2));
        assert_eq!(array_vec.pop(), Some(1));

        assert!(array_vec.last().is_none());
    }
}
