use std::fmt::Debug;
use std::net::SocketAddr;
use std::io::Result;
use std::time::Instant;
use crate::constants::{PACKET_LOST_CUTOFF, PL_SMOOTHING_FACTOR, RTT_SMOOTHING_FACTOR};
use crate::MAX_PACKET_SIZE;
use crate::packets::Packet;
use crate::sequencing::{SequenceBuffer, SequenceNumber, SequenceNumberSet, SequenceResult};
use crate::socket::Transport;

#[derive(Debug)]
pub struct PacketSocket {
    socket: Box<dyn Transport>,
    buffer: [u8; MAX_PACKET_SIZE],
    salt: String
}

impl PacketSocket {

    pub fn new<T: Transport + 'static>(socket: T, identifier: &str) -> Self {
        Self {
            socket: Box::new(socket),
            buffer: [0; MAX_PACKET_SIZE],
            salt: identifier.to_string()
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn recv_from(&mut self) -> Result<(Result<Packet>, SocketAddr)> {
        let (size, src) = self.socket.recv_from(&mut self.buffer)?;
        Ok((Packet::from(&self.buffer[..size], self.salt.as_bytes()), src))
    }

    pub fn send_to(&mut self, packet: Packet, addrs: SocketAddr) -> Result<()> {
        let packet = packet.write(&mut self.buffer, self.salt.as_bytes())?;
        let i = self.socket.send_to(packet, addrs)?;
        assert_eq!(packet.len(), i);
        Ok(())
    }

    pub fn send_with(&mut self, packet: Packet, connection: &mut VirtualConnection) -> Result<()> {
        connection.last_send_packet = Instant::now();
        self.send_to(packet, connection.addrs)
    }

    pub fn send_payload(&mut self, payload: &[u8], connection: &mut VirtualConnection) -> Result<SequenceNumber> {
        let seq = connection.next_sequence_number();
        let ack = connection.received_packets;
        self.send_with(Packet::Payload(seq, ack, payload), connection)?;
        Ok(seq)
    }

    pub fn send_keepalive(&mut self, connection: &mut VirtualConnection) -> Result<()> {
        let ack = connection.received_packets;
        self.send_with(Packet::KeepAlive(ack), connection)
    }

}

#[derive(Clone, Debug)]
struct PacketInformation{
    send_time: Instant
}

impl PacketInformation {
    fn new() -> Self {
        Self {
            send_time: Instant::now()
        }
    }
}

#[derive(Debug, Clone)]
pub struct VirtualConnection {
    addrs: SocketAddr,
    id: u16,
    pub last_received_packet: Instant,
    pub last_send_packet: Instant,
    received_packets: SequenceNumberSet,
    sent_packets: SequenceBuffer<PacketInformation>,
    rtt: f32,
    packet_loss: f32
}

impl VirtualConnection {
    pub fn new(addrs: SocketAddr, id: u16) -> Self {
        Self {
            addrs,
            id,
            last_received_packet: Instant::now(),
            last_send_packet: Instant::now(),
            received_packets: SequenceNumberSet::new(0),
            sent_packets: SequenceBuffer::with_capacity(1024),
            rtt: 0.0,
            packet_loss: 0.0
        }
    }

    pub fn id(&self) -> u16 {
        self.id
    }

    pub fn addrs(&self) -> SocketAddr {
        self.addrs
    }

    pub fn rtt(&self) -> u32 {
        f32::round(self.rtt * 1000.0) as u32
    }

    pub fn packet_loss(&self) -> f32 {
        (self.packet_loss * 1000.0).round() / 1000.0
    }

    pub fn on_receive(&mut self) {
        self.last_received_packet = Instant::now();
    }

    pub fn handle_seq(&mut self, seq: SequenceNumber) -> Option<bool> {
        match self.received_packets.insert(seq) {
            SequenceResult::Latest => Some(true),
            SequenceResult::Fresh => Some(false),
            SequenceResult::Duplicate => None,
            SequenceResult::TooOld => None
        }
    }

    pub fn handle_ack<F>(&mut self, ack: SequenceNumberSet, mut callback: F) where F: FnMut(SequenceNumber) {

        for (_seq, _) in self.sent_packets.drain_older(ack.latest().wrapping_sub(PACKET_LOST_CUTOFF)) {
            self.packet_loss = lerp(self.packet_loss, 1., PL_SMOOTHING_FACTOR);
        }
        for seq in ack.iter() {
            if let Some(info) = self.sent_packets.remove(seq) {
                callback(seq);
                let rtt = info.send_time.elapsed().as_secs_f32();
                self.rtt = lerp(self.rtt, rtt, RTT_SMOOTHING_FACTOR);

                self.packet_loss = lerp(self.packet_loss, 0., PL_SMOOTHING_FACTOR);
            }
        }
    }

    pub fn peek_next_sequence_number(&self) -> SequenceNumber {
        self.sent_packets.next_sequence_number()
    }

    pub fn next_sequence_number(&mut self) -> SequenceNumber {
        let (seq, _) = self.sent_packets.insert(PacketInformation::new());
        seq
    }

}

fn lerp(a: f32, b: f32, v: f32) -> f32 {
    a + (b - a) * v
}