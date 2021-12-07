use std::io::Write;
use byteorder::{WriteBytesExt, NetworkEndian, ReadBytesExt};

#[derive(Debug)]
pub struct MessageChannel {
    pub local_sequence_number: u16,
    pub remote_sequence_number: u16,
    pub remote_sequence_bitfield: u32,
    pub send_items: Vec<u16>
}



impl MessageChannel {

    pub fn new() -> Self {
        Self {
            local_sequence_number: 0,
            remote_sequence_number: 0,
            remote_sequence_bitfield: 0,
            send_items: Vec::new()
        }
    }

    pub fn read(&mut self, data: &[u8]) -> Vec<Box<[u8]>> {
        let mut packet = data;
        let sequence = packet.read_u16::<NetworkEndian>().unwrap();
        let ack = packet.read_u16::<NetworkEndian>().unwrap();
        let ack_bitfield = packet.read_u32::<NetworkEndian>().unwrap();
        if sequence > self.remote_sequence_number {
            self.remote_sequence_bitfield <<= 1;
            self.remote_sequence_bitfield |= 0x1;
            self.remote_sequence_bitfield <<= (sequence - self.remote_sequence_number - 1);
            self.remote_sequence_number = sequence;
            self.send_items.retain(|i| {
                if *i == ack {
                    false
                } else {
                    let diff = ack.saturating_sub(*i);
                    if (1..=32).contains(&diff) {
                        (ack_bitfield >> (diff - 1)) & 0x1 == 0x0
                    } else {
                        true
                    }
                }
            });
            vec![packet.into()]
        } else {
            vec![]
        }



    }

    pub fn send(&mut self, data: &[u8]) -> Box<[u8]> {
        let mut packet = Vec::new();
        self.local_sequence_number += 1;
        packet.write_u16::<NetworkEndian>(self.local_sequence_number).unwrap();
        self.send_items.push(self.local_sequence_number);
        packet.write_u16::<NetworkEndian>(self.remote_sequence_number).unwrap();
        packet.write_u32::<NetworkEndian>(self.remote_sequence_bitfield).unwrap();
        packet.write_all(data).unwrap();
        packet.into_boxed_slice()

    }

}