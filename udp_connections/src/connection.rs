use std::net::{SocketAddr, ToSocketAddrs};
use std::io::Result;
use std::time::Instant;
use crate::MAX_PACKET_SIZE;
use crate::packets::Packet;
use crate::sequencing::{SequenceBuffer, SequenceNumber, SequenceNumberSet};
use crate::socket::UdpSocketImpl;

#[derive(Debug)]
pub struct PacketSocket<U: UdpSocketImpl> {
    socket: U,
    buffer: [u8; MAX_PACKET_SIZE],
    salt: String
}

impl<U> PacketSocket<U> where U: UdpSocketImpl {

    pub fn new(socket: U, identifier: &str) -> Self {
        Self {
            socket,
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

    pub fn send_to<A: ToSocketAddrs>(&mut self, packet: Packet, addrs: A) -> Result<()> {
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
        let seq = connection.sent_packets.insert(PacketInformation);
        let ack = connection.received_packets;
        self.send_with(Packet::Payload(seq, ack, payload), connection)?;
        Ok(seq)
    }

}

#[derive(Clone, Default)]
struct PacketInformation;

#[derive(Debug, Clone)]
pub struct VirtualConnection {
    pub addrs: SocketAddr,
    pub id: u16,
    pub last_received_packet: Instant,
    pub last_send_packet: Instant,
    received_packets: SequenceNumberSet,
    sent_packets: SequenceBuffer<PacketInformation>
}

impl VirtualConnection {
    pub fn new(addrs: SocketAddr, id: u16) -> Self {
        Self {
            addrs,
            id,
            last_received_packet: Instant::now(),
            last_send_packet: Instant::now(),
            received_packets: SequenceNumberSet::new(0),
            sent_packets: SequenceBuffer::with_capacity(64)
        }
    }

    pub fn on_receive(&mut self) {
        self.last_received_packet = Instant::now();
    }

    pub fn handle_seq(&mut self, seq: SequenceNumber) {
        self.received_packets.insert(seq)
    }

    pub fn handle_ack<F>(&mut self, ack: SequenceNumberSet, mut callback: F) where F: FnMut(SequenceNumber) {
        for seq in ack.iter() {
            if let Some(_) = self.sent_packets.remove(seq) {
                callback(seq);
            }
        }
    }

}

