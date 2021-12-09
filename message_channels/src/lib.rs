mod sequencing;

use std::fmt::Debug;
use std::io::Write;
use byteorder::{WriteBytesExt, NetworkEndian, ReadBytesExt};
use crate::sequencing::{SequenceBuffer, SequenceNumberSet};

#[derive(Debug)]
pub struct MessageChannel {
    pub local_sequence_number: u16,
    pub remote_sequence: SequenceNumberSet,
    pub send_items: SequenceBuffer<SendPacket>
}

#[derive(Clone, Default)]
pub struct SendPacket;

impl MessageChannel {

    pub fn new() -> Self {
        Self {
            local_sequence_number: 0,
            remote_sequence: SequenceNumberSet::new(0),
            send_items: SequenceBuffer::with_capacity(1024)
        }
    }

    pub fn read(&mut self, data: &[u8]) -> Vec<Box<[u8]>> {
        let mut packet = data;
        let sequence = packet.read_u16::<NetworkEndian>().unwrap();
        let ack = SequenceNumberSet::from_bitfield(
            packet.read_u16::<NetworkEndian>().unwrap(),
            packet.read_u32::<NetworkEndian>().unwrap());

        self.remote_sequence.insert(sequence);
        for seq in ack.iter() {
            self.send_items.remove(seq);
        }
        vec![packet.into()]

    }

    pub fn send(&mut self, data: &[u8]) -> Box<[u8]> {
        let mut packet = Vec::new();
        self.local_sequence_number += 1;
        packet.write_u16::<NetworkEndian>(self.local_sequence_number).unwrap();
        self.send_items.insert(self.local_sequence_number, SendPacket);
        packet.write_u16::<NetworkEndian>(self.remote_sequence.latest()).unwrap();
        packet.write_u32::<NetworkEndian>(self.remote_sequence.bitfield()).unwrap();
        packet.write_all(data).unwrap();
        packet.into_boxed_slice()

    }

}

