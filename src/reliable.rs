use std::io::{Error, ErrorKind, Read, Write};
use std::io::Result;
use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use crate::sequencing::{sequence_greater_than, sequence_less_than, SequenceBuffer, SequenceNumber};

#[derive(Clone, Default)]
struct Message {
    data: Box<[u8]>,
    sequence_number: Vec<SequenceNumber>
}

#[derive(Debug)]
pub struct MessageChannel {
    buffer: Vec<u8>,
    outgoing_messages: SequenceBuffer<Message>,
    incoming_messages: SequenceBuffer2<Box<[u8]>>,
    last_read_message: SequenceNumber
}

impl MessageChannel {

    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            outgoing_messages: SequenceBuffer::with_capacity(256),
            incoming_messages: SequenceBuffer2::with_capacity(256),
            last_read_message: 0
        }
    }

    pub fn receive_message(&mut self) -> Option<Box<[u8]>> {
        let next = self.last_read_message.wrapping_add(1);
        match self.incoming_messages.remove(next){
            None => None,
            Some(msg) => {
                self.last_read_message = next;
                Some(msg)
            }
        }
    }

    pub fn queue_message(&mut self, msg: &[u8]) -> Result<()>{
        match self.outgoing_messages.try_insert(Message {
            data: msg.into(),
            sequence_number: Vec::new()
        }) {
            None => Err(Error::new(ErrorKind::Other, "can not queue any more messages")),
            Some(_) => Ok(())
        }
    }

    pub fn on_receive(&mut self, mut packet: &[u8]) -> Result<()> {
        let len = packet.read_u8()?;
        for _ in 0..len {
            let msg_id = packet.read_u16::<NetworkEndian>()?;
            let size = packet.read_u8()? as usize;
            if sequence_less_than(self.last_read_message, msg_id) && !self.incoming_messages.exists(msg_id) {
                let mut buf = vec![0u8; size].into_boxed_slice();
                packet.read_exact(buf.as_mut())?;
                self.incoming_messages.insert(msg_id, buf);
            } else {
                for _ in 0..size {
                    packet.read_u8()?;
                }
            }
        }
        Ok(())
    }

    pub fn on_ack(&mut self, seq: SequenceNumber) {
        let mut tmp = Vec::new();
        for (id, msg) in self.outgoing_messages.iter_mut() {
            if msg.sequence_number.contains(&seq) {
                tmp.push(id);
            }
        }

        for id in tmp {
            self.outgoing_messages.remove(id);
        }
    }

    pub fn send_packets(&mut self, seq: SequenceNumber) -> Result<&[u8]> {
        let packet = &mut self.buffer;

        packet.clear();
        packet.write_u8(0)?;

        for (id, msg) in self.outgoing_messages.iter_mut().take(5) {
            packet[0] += 1;
            packet.write_u16::<NetworkEndian>(id)?;
            packet.write_u8(msg.data.len() as u8)?;
            packet.write_all(msg.data.as_ref())?;
            msg.sequence_number.push(seq);
        }

        Ok(packet.as_slice())
    }

    pub fn has_unsend_messages(&self) -> bool {
        !self.outgoing_messages.is_empty()
    }

}

#[derive(Debug)]
pub struct SequenceBuffer2<T: Clone + Default> {
    sequence_num: SequenceNumber,
    entry_sequences: Box<[Option<SequenceNumber>]>,
    entries: Box<[T]>,
}

impl<T: Clone + Default> SequenceBuffer2<T> {
    pub fn with_capacity(size: u16) -> Self {
        Self {
            sequence_num: 0,
            entry_sequences: vec![None; size as usize].into_boxed_slice(),
            entries: vec![T::default(); size as usize].into_boxed_slice(),
        }
    }

    pub fn insert(&mut self, sequence_num: SequenceNumber, entry: T) -> Option<&mut T> {
        if sequence_less_than(
            sequence_num,
            self.sequence_num
                .wrapping_sub(self.entry_sequences.len() as u16),
        ) {
            return None;
        }

        self.advance_sequence(sequence_num);

        let index = self.index(sequence_num);
        self.entry_sequences[index] = Some(sequence_num);
        self.entries[index] = entry;
        Some(&mut self.entries[index])
    }

    pub fn exists(&self, sequence_num: SequenceNumber) -> bool {
        let index = self.index(sequence_num);
        if let Some(s) = self.entry_sequences[index] {
            return s == sequence_num;
        }
        false
    }

    pub fn remove(&mut self, sequence_num: SequenceNumber) -> Option<T> {
        if self.exists(sequence_num) {
            let index = self.index(sequence_num);
            let value = std::mem::take(&mut self.entries[index]);
            self.entry_sequences[index] = None;
            return Some(value);
        }
        None
    }

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

    fn index(&self, sequence: SequenceNumber) -> usize {
        sequence as usize % self.entry_sequences.len()
    }
}