use core::ops::{Deref, DerefMut};

#[derive(Debug, PartialEq, Eq)]
pub enum ArrayVecError<T> {
    NoSpaceLeft(T),
}

pub struct ArrayVec<T, const LENGTH: usize> {
    elements: [core::mem::MaybeUninit<T>; LENGTH],
    length: usize,
}

impl<T, const LENGTH: usize> ArrayVec<T, LENGTH> {
    pub const fn new() -> Self {
        Self {
            elements: [const { core::mem::MaybeUninit::uninit() }; LENGTH],
            length: 0,
        }
    }

    pub fn push(&mut self, element: T) -> Result<(), ArrayVecError<T>> {
        if self.length == LENGTH {
            return Err(ArrayVecError::NoSpaceLeft(element));
        }
        self.elements[self.length].write(element);
        self.length += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.length == 0 {
            return None;
        }
        self.length -= 1;
        // SAFETY: length defines if an element was assigned or not
        unsafe { Some(self.elements[self.length].assume_init_read()) }
    }

    pub fn iter(&self) -> ArrayVecIter<'_, T, LENGTH> {
        ArrayVecIter {
            elements: self,
            position: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T, const LENGTH: usize> Default for ArrayVec<T, LENGTH> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const LENGTH: usize> Drop for ArrayVec<T, LENGTH> {
    fn drop(&mut self) {
        for index in 0..self.length {
            // SAFETY: All elements in [0..self.length) are initialized (maintained by push/pop).
            unsafe {
                self.elements[index].assume_init_drop();
            }
        }
    }
}

pub struct ArrayVecIter<'a, T, const LENGTH: usize> {
    elements: &'a ArrayVec<T, LENGTH>,
    position: usize,
}

impl<'a, T, const LENGTH: usize> Iterator for ArrayVecIter<'a, T, LENGTH> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.elements.length {
            return None;
        }
        // SAFETY: We know this element is initialized because its index
        // is less than length
        let element = unsafe { self.elements.elements[self.position].assume_init_ref() };
        self.position += 1;
        Some(element)
    }
}

impl<'a, T, const LENGTH: usize> IntoIterator for &'a ArrayVec<T, LENGTH> {
    type Item = &'a T;

    type IntoIter = ArrayVecIter<'a, T, LENGTH>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T, const LENGTH: usize> Deref for ArrayVec<T, LENGTH> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        // SAFETY: All elements in [0..self.length) are initialized (maintained by push/pop).
        unsafe { self.elements[..self.length].assume_init_ref() }
    }
}

impl<T, const LENGTH: usize> DerefMut for ArrayVec<T, LENGTH> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: All elements in [0..self.length) are initialized (maintained by push/pop).
        unsafe { self.elements[..self.length].assume_init_mut() }
    }
}
