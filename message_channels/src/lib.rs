mod sequencing;

use std::fmt::Debug;
use std::io::Write;
use byteorder::{WriteBytesExt, NetworkEndian, ReadBytesExt};
use crate::sequencing::{SequenceBuffer, SequenceNumber, SequenceNumberSet};

#[derive(Debug)]
pub struct MessageChannel {
    received_packets: SequenceNumberSet,
    sent_packets: SequenceBuffer<SendPacket>
}

#[derive(Clone, Default)]
pub struct SendPacket;

impl MessageChannel {

    pub fn new() -> Self {
        Self {
            received_packets: SequenceNumberSet::new(0),
            sent_packets: SequenceBuffer::with_capacity(64)
        }
    }

    pub fn read<F>(&mut self, data: &[u8], f: F) -> Vec<Box<[u8]>> where F: Fn(SequenceNumber){
        let mut packet = data;
        let sequence = packet.read_u16::<NetworkEndian>().unwrap();
        let ack_packets = SequenceNumberSet::from_bitfield(
            packet.read_u16::<NetworkEndian>().unwrap(),
            packet.read_u32::<NetworkEndian>().unwrap());

        self.received_packets.insert(sequence);
        for seq in ack_packets.iter() {
            if let Some(_) = self.sent_packets.remove(seq) {
                f(seq);
            }
        }
        vec![packet.into()]

    }

    pub fn send(&mut self, data: &[u8]) -> Box<[u8]> {
        let mut packet = Vec::new();
        let sequence = self.sent_packets.insert(SendPacket);
        packet.write_u16::<NetworkEndian>(sequence).unwrap();
        packet.write_u16::<NetworkEndian>(self.received_packets.latest()).unwrap();
        packet.write_u32::<NetworkEndian>(self.received_packets.bitfield()).unwrap();
        packet.write_all(data).unwrap();
        packet.into_boxed_slice()

    }

}

