use std::fmt::{Debug, Formatter};

pub type SequenceNumber = u16;

#[derive(Clone)]
pub struct SequenceBuffer<T> {
    newest_sequence_number: SequenceNumber,
    len: usize,
    entries: Box<[Option<T>]>
}

impl<T: Clone> Debug for SequenceBuffer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.iter().map(|(i, _)| i)).finish()
    }
}

impl<T: Clone> SequenceBuffer<T> {

    pub fn with_capacity(size: usize) -> Self {
        Self {
            newest_sequence_number: 0,
            len: 0,
            entries: vec![None; size].into_boxed_slice()
        }
    }

    pub fn insert(&mut self, data: T) -> (SequenceNumber, Option<T>) {
        let prev = self.remove_at(self.index(self.next_sequence_number()));
        (self.try_insert(data).expect("This should never fail"), prev)
    }

    pub fn try_insert(&mut self, data: T) -> Option<SequenceNumber> {
        let sequence_number = self.next_sequence_number();
        let index = self.index(sequence_number);
        match self.entries[index] {
            ref mut entry @ None => {
                self.newest_sequence_number = sequence_number;
                *entry = Some(data);
                self.len += 1;
                debug_assert!(self.len <= self.entries.len());
                Some(sequence_number)
            }
            Some(_) => None
        }
    }

    pub fn remove(&mut self, sequence: SequenceNumber) -> Option<T> {
        if sequence_greater_than(sequence, self.newest_sequence_number)
            || sequence_less_than(sequence, self.oldest_sequence_number()) {
            None
        } else {
            self.remove_at(self.index(sequence))
        }

    }

    fn remove_at(&mut self, index: usize) -> Option<T> {
        let old = self.entries[index].take();
        while self.entries[self.index(self.oldest_sequence_number())].is_none() && self.len > 0 {
            self.len -= 1;
        }
        old
    }

    fn oldest_sequence_number(&self) -> SequenceNumber {
        self.newest_sequence_number
            .wrapping_sub(self.len as SequenceNumber)
            .wrapping_add(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter_mut(&mut self) -> SequenceBufferIterMut<T> {
        SequenceBufferIterMut {
            inner: self,
            index: 0
        }
    }

    pub fn iter(&self) -> SequenceBufferIter<T> {
        SequenceBufferIter {
            inner: self,
            index: 0
        }
    }

    pub fn next_sequence_number(&self) -> SequenceNumber {
        self.newest_sequence_number.wrapping_add(1)
    }

    fn index(&self, sequence: SequenceNumber) -> usize {
        sequence as usize % self.entries.len()
    }

}

pub struct SequenceBufferIter<'a, T> {
    inner: &'a SequenceBuffer<T>,
    index: usize
}

impl<'a, T: Clone> Iterator for SequenceBufferIter<'a, T> {
    type Item = (SequenceNumber, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.inner;
        while self.index < b.len {
            self.index += 1;
            let seq = b.newest_sequence_number.wrapping_sub((b.len - self.index) as SequenceNumber);
            unsafe {
                match b.entries.get_unchecked(b.index(seq)) {
                    None => continue,
                    Some(data) => return Some((seq, data))
                }
            }
        }
        None
    }
}

pub struct SequenceBufferIterMut<'a, T: 'a> {
    inner: &'a mut SequenceBuffer<T>,
    index: usize
}

impl<'a, T: Clone + 'a> Iterator for SequenceBufferIterMut<'a, T>{
    type Item = (SequenceNumber, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.inner.len {
            self.index += 1;
            let seq = self.inner.newest_sequence_number.wrapping_sub((self.inner.len - self.index) as SequenceNumber);
            let index = self.inner.index(seq);
            unsafe {
                let elem = self.inner.entries.get_unchecked_mut(index);
                // and now for some black magic
                // the std stuff does this too, but afaik this breaks the borrow checker
                match elem {
                    None => continue,
                    Some(data) => return Some((seq, &mut *(data as *mut T)))
                }
            }

        }
        None
    }
}

pub fn sequence_greater_than(s1: SequenceNumber, s2: SequenceNumber) -> bool {
    const HALF: SequenceNumber = SequenceNumber::MAX / 2;
    ((s1 > s2) && (s1 - s2 <= HALF)) || ((s1 < s2) && (s2 - s1 > HALF))
}

pub fn sequence_less_than(s1: SequenceNumber, s2: SequenceNumber) -> bool {
    sequence_greater_than(s2, s1)
}

type SequenceBitfield = u32;

#[derive(Copy, Clone, PartialEq)]
pub struct SequenceNumberSet {
    latest: SequenceNumber,
    bitfield: SequenceBitfield
}

impl SequenceNumberSet {

    pub fn from_bitfield(latest: SequenceNumber, bitfield: SequenceBitfield) -> Self {
        Self {
            latest,
            bitfield
        }
    }

    pub fn new(sequence: SequenceNumber) -> Self{
        Self::from_bitfield(sequence, 0)
    }

    pub const fn capacity() -> usize {
        SequenceBitfield::BITS as usize + 1
    }

    pub fn latest(self) -> SequenceNumber {
        self.latest
    }

    pub fn bitfield(self) -> SequenceBitfield {
        self.bitfield
    }

    pub fn contains(self, sequence: SequenceNumber) -> bool {
        match self.latest == sequence {
            true => true,
            false => match self.index(sequence) {
                None => false,
                Some(index) => (self.bitfield >> index) & 0b1 == 0b1
            }
        }
    }

    pub fn insert(&mut self, sequence: SequenceNumber) -> bool {
        match sequence_greater_than(sequence, self.latest) {
            true => {
                let offset = sequence.wrapping_sub(self.latest);
                self.bitfield <<= 1;
                self.bitfield |= 0b1;
                self.bitfield <<= offset - 1;
                self.latest = sequence;
                true
            }
            false => match self.index(sequence) {
                None => false,
                Some(index) => {
                    let old = self.bitfield;
                    self.bitfield |= 0b1 << index;
                    self.bitfield != old
                }
            }
        }
    }

    fn index(self, sequence: SequenceNumber) -> Option<usize> {
        if sequence_less_than(sequence, self.latest) {
            let offset: usize = self.latest.wrapping_sub(sequence).into();
            if offset >= Self::capacity() {
                return None
            }
            return Some(offset - 1)
        }
        None
    }

    pub fn iter(self) -> impl Iterator<Item=SequenceNumber> {
        (0..Self::capacity())
            .rev()
            .map(move |i|self.latest.wrapping_sub(i as SequenceNumber))
            .filter(move |i|self.contains(*i))
    }

}

impl Debug for SequenceNumberSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {

    mod sequence_buffer {
        use crate::sequencing::SequenceBuffer;

        #[test]
        fn test_insert_remove() {
            let mut buffer = SequenceBuffer::with_capacity(4);
            assert!(buffer.is_empty());
            assert_eq!(buffer.remove(0), None);
            let (s1, _) = buffer.insert(1);
            assert_eq!(buffer.remove(s1), Some(1));
            assert_eq!(buffer.remove(s1), None);

            let (s1, _) = buffer.insert(1);
            assert!(!buffer.is_empty());
            let (s2, _) = buffer.insert(2);
            assert!(!buffer.is_empty());
            assert_eq!(buffer.remove(s1), Some(1));
            assert_eq!(buffer.remove(s2), Some(2));

            let (s1, _) = buffer.insert(1);
            let _ = buffer.insert(2);
            let _ = buffer.insert(3);
            assert!(buffer.try_insert(4).is_some());
            assert!(buffer.try_insert(5).is_none());
            let (_, old) = buffer.insert(5);
            assert_eq!(old, Some(1));
            assert_eq!(buffer.remove(s1), None);
        }

        #[test]
        fn test_iter() {
            let mut buffer = SequenceBuffer::with_capacity(4);
            buffer.insert(1);
            buffer.insert(2);
            buffer.insert(3);

            let mut iter = buffer.iter().map(|(i,_)|i);
            assert_eq!(iter.next(), Some(1));
            assert_eq!(iter.next(), Some(2));
            assert_eq!(iter.next(), Some(3));
            assert_eq!(iter.next(), None);

            buffer.insert(4);
            buffer.insert(5);
            buffer.insert(6);

            let mut iter = buffer.iter().map(|(i,_)|i);
            assert_eq!(iter.next(), Some(3));
            assert_eq!(iter.next(), Some(4));
            assert_eq!(iter.next(), Some(5));
            assert_eq!(iter.next(), Some(6));
            assert_eq!(iter.next(), None);
        }

        #[test]
        fn test_iter_mut() {
            let mut buffer = SequenceBuffer::with_capacity(4);
            buffer.insert(1);
            buffer.insert(2);
            buffer.insert(3);

            let mut iter = buffer.iter_mut().map(|(i,_)|i);
            assert_eq!(iter.next(), Some(1));
            assert_eq!(iter.next(), Some(2));
            assert_eq!(iter.next(), Some(3));
            assert_eq!(iter.next(), None);

            buffer.insert(4);
            buffer.insert(5);
            buffer.insert(6);

            let mut iter = buffer.iter_mut().map(|(i,_)|i);
            assert_eq!(iter.next(), Some(3));
            assert_eq!(iter.next(), Some(4));
            assert_eq!(iter.next(), Some(5));
            assert_eq!(iter.next(), Some(6));
            assert_eq!(iter.next(), None);
        }

    }

    mod sequence_set {
        use crate::sequencing::{SequenceNumber, SequenceNumberSet};

        #[test]
        fn test_contains() {
            let set = SequenceNumberSet::from_bitfield(3, 0b000010001);
            assert!(!set.contains(4));
            assert!( set.contains(3));
            assert!( set.contains(2));
            assert!(!set.contains(1));
            assert!(!set.contains(0));
            assert!(!set.contains(SequenceNumber::MAX - 0));
            assert!( set.contains(SequenceNumber::MAX - 1));
            assert!(!set.contains(SequenceNumber::MAX - 2));
            assert!(!set.contains(SequenceNumber::MAX - 3));
        }

        #[test]
        fn test_insert() {
            let mut set = SequenceNumberSet::new(0);
            assert_eq!(set.latest(), 0);
            assert_eq!(set.bitfield(), 0b0);

            assert!(set.insert(5));
            assert_eq!(set.latest(), 5);
            assert_eq!(set.bitfield(), 0b10000);

            assert!(set.insert(7));
            assert_eq!(set.latest(), 7);
            assert_eq!(set.bitfield(), 0b1000010);

            assert!(set.insert(3));
            assert_eq!(set.latest(), 7);
            assert_eq!(set.bitfield(), 0b1001010);

            assert!(!set.insert(3));
            assert!(!set.insert(SequenceNumber::MAX - 40));
        }

        #[test]
        fn test_iter() {
            let mut iter = SequenceNumberSet::from_bitfield(3, 0b000010001).iter();
            assert_eq!(iter.next(), Some(SequenceNumber::MAX - 1));
            assert_eq!(iter.next(), Some(2));
            assert_eq!(iter.next(), Some(3));
            assert_eq!(iter.next(), None);

        }

    }

}
