use std::fmt::{Debug, Formatter};

type SequenceNumber = u16;

pub struct SequenceBuffer<T: Clone + Default> {
    sequence_num: SequenceNumber,
    entry_sequences: Box<[Option<SequenceNumber>]>,
    entries: Box<[T]>,
}

impl<T: Clone + Default> Debug for SequenceBuffer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut iter = self.entry_sequences.iter().filter_map(|i|*i);
        write!(f, "[")?;
        if let Some(arg) = iter.next() {
            write!(f, "{}", arg)?;

            for arg in iter {
                write!(f, " {}", arg)?;
            }
        }
        write!(f, "]")?;
        Ok(())
    }
}

impl<T: Clone + Default> SequenceBuffer<T> {
    /// Creates a SequenceBuffer with a desired capacity.
    pub fn with_capacity(size: u16) -> Self {
        Self {
            sequence_num: 0,
            entry_sequences: vec![None; size as usize].into_boxed_slice(),
            entries: vec![T::default(); size as usize].into_boxed_slice(),
        }
    }

    /// Returns the most recently stored sequence number.
    pub fn sequence_num(&self) -> SequenceNumber {
        self.sequence_num
    }

    /// Returns a mutable reference to the entry with the given sequence number.
    pub fn get_mut(&mut self, sequence_num: SequenceNumber) -> Option<&mut T> {
        if self.exists(sequence_num) {
            let index = self.index(sequence_num);
            Some(&mut self.entries[index])
        } else {
            None
        }
    }

    /// Inserts the entry data into the sequence buffer. If the requested sequence number is "too
    /// old", the entry will not be inserted and no reference will be returned.
    pub fn insert(&mut self, sequence_num: SequenceNumber, entry: T) -> Option<&mut T> {
        // sequence number is too old to insert into the buffer
        if sequence_less_than(
            sequence_num,
            self.sequence_num.wrapping_sub(self.entry_sequences.len() as u16)) {
            return None;
        }

        self.advance_sequence(sequence_num);

        let index = self.index(sequence_num);
        self.entry_sequences[index] = Some(sequence_num);
        self.entries[index] = entry;
        Some(&mut self.entries[index])
    }

    /// Returns whether or not we have previously inserted an entry for the given sequence number.
    pub fn exists(&self, sequence_num: SequenceNumber) -> bool {
        match self.entry_sequences[self.index(sequence_num)] {
            None => false,
            Some(s) => s == sequence_num
        }
    }

    /// Removes an entry from the sequence buffer
    pub fn remove(&mut self, sequence_num: SequenceNumber) -> Option<T> {
        if self.exists(sequence_num) {
            let index = self.index(sequence_num);
            let value = std::mem::take(&mut self.entries[index]);
            self.entry_sequences[index] = None;
            return Some(value);
        }
        None
    }

    // Advances the sequence number while removing older entries.
    fn advance_sequence(&mut self, sequence_num: SequenceNumber) {
        if sequence_greater_than(sequence_num.wrapping_add(1), self.sequence_num) {
            self.remove_entries(u32::from(sequence_num));
            self.sequence_num = sequence_num.wrapping_add(1);
        }
    }

    fn remove_entries(&mut self, mut finish_sequence: u32) {
        let start_sequence = u32::from(self.sequence_num);
        if finish_sequence < start_sequence {
            finish_sequence += 65536;
        }

        if finish_sequence - start_sequence < self.entry_sequences.len() as u32 {
            for sequence in start_sequence..=finish_sequence {
                self.remove(sequence as u16);
            }
        } else {
            for index in 0..self.entry_sequences.len() {
                self.entries[index] = T::default();
                self.entry_sequences[index] = None;
            }
        }
    }

    // Generates an index for use in `entry_sequences` and `entries`.
    fn index(&self, sequence: SequenceNumber) -> usize {
        sequence as usize % self.entry_sequences.len()
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

#[derive(Copy, Clone)]
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
