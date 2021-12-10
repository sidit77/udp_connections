use std::fmt::{Debug, Formatter};
pub type SequenceNumber = u16;

#[derive(Clone)]
pub struct SequenceBuffer<T> {
    last_sequence_number: SequenceNumber,
    entries: Box<[Option<(SequenceNumber, T)>]>
}

impl<T> Debug for SequenceBuffer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut iter = self.entries
            .iter()
            .filter_map(|i|i.as_ref().map(|(j, _)|*j));
        write!(f, "[")?;
        if let Some(arg) = iter.next() {
            write!(f, "{}", arg)?;
            for arg in iter {
                write!(f, ", {}", arg)?;
            }
        }
        write!(f, "]")?;
        Ok(())
    }
}

impl<T: Clone> SequenceBuffer<T> {

    pub fn with_capacity(size: usize) -> Self {
        Self {
            last_sequence_number: 0,
            entries: vec![None; size].into_boxed_slice()
        }
    }

    pub fn insert(&mut self, data: T) -> SequenceNumber {
        let sequence_number = self.next_sequence_number();
        let index = self.index(sequence_number);
        self.entries[index] = Some((sequence_number, data));
        sequence_number
    }

    pub fn remove(&mut self, sequence: SequenceNumber) -> Option<T> {
        match &mut self.entries[self.index(sequence)] {
            None => None,
            Some((s, _)) if *s != sequence => None,
            elem => match elem.take() {
                None => unreachable!(),
                Some((_, data)) => Some(data)
            }
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item=(SequenceNumber, &mut T)> {
        self.entries.iter_mut().filter_map(|e|e.as_mut().map(|(i, t)|(*i, t)))
    }

    pub fn iter(&self) -> impl Iterator<Item=(SequenceNumber, &T)> {
        self.entries.iter().filter_map(|e|e.as_ref().map(|(i, t)|(*i, t)))
    }

    fn next_sequence_number(&mut self) -> SequenceNumber {
        self.last_sequence_number = self.last_sequence_number.wrapping_add(1);
        self.last_sequence_number
    }

    fn index(&self, sequence: SequenceNumber) -> usize {
        sequence as usize % self.entries.len()
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

    pub fn insert(&mut self, sequence: SequenceNumber) {
        match sequence_greater_than(sequence, self.latest) {
            true => {
                let offset = sequence.wrapping_sub(self.latest);
                self.bitfield <<= 1;
                self.bitfield |= 0b1;
                self.bitfield <<= offset - 1;
                self.latest = sequence;
            }
            false => match self.index(sequence) {
                None => {}
                Some(index) => self.bitfield |= 0b1 << index
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
        let mut iter = self.iter();
        write!(f, "[")?;
        if let Some(arg) = iter.next() {
            write!(f, "{}", arg)?;
            for arg in iter {
                write!(f, ", {}", arg)?;
            }
        }
        write!(f, "]")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    mod sequence_buffer {
        use crate::sequencing::SequenceBuffer;

        #[test]
        fn test_insert_remove() {
            let mut buffer = SequenceBuffer::with_capacity(4);
            assert_eq!(buffer.remove(0), None);
            let s1 = buffer.insert(1);
            assert_eq!(buffer.remove(s1), Some(1));
            assert_eq!(buffer.remove(s1), None);

            let s1 = buffer.insert(1);
            let s2 = buffer.insert(2);
            assert_eq!(buffer.remove(s1), Some(1));
            assert_eq!(buffer.remove(s2), Some(2));

            let s1 = buffer.insert(1);
            let _ = buffer.insert(2);
            let _ = buffer.insert(3);
            let _ = buffer.insert(4);
            let _ = buffer.insert(5);
            assert_eq!(buffer.remove(s1), None);

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

            set.insert(5);
            assert_eq!(set.latest(), 5);
            assert_eq!(set.bitfield(), 0b10000);

            set.insert(7);
            assert_eq!(set.latest(), 7);
            assert_eq!(set.bitfield(), 0b1000010);

            set.insert(3);
            assert_eq!(set.latest(), 7);
            assert_eq!(set.bitfield(), 0b1001010);
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
