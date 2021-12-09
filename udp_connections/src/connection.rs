use std::net::{SocketAddr, ToSocketAddrs};
use std::io::Result;
use std::time::Instant;
use crate::MAX_PACKET_SIZE;
use crate::packets::Packet;
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
        connection.on_send();
        self.send_to(packet, connection.addrs)
    }

}

#[derive(Debug, Clone, Copy)]
pub struct VirtualConnection {
    pub addrs: SocketAddr,
    pub id: u16,
    pub last_received_packet: Instant,
    pub last_send_packet: Instant,
    pub should_disconnect: bool
}

impl VirtualConnection {
    pub fn new(addrs: SocketAddr, id: u16) -> Self {
        Self {
            addrs,
            id,
            last_received_packet: Instant::now(),
            last_send_packet: Instant::now(),
            should_disconnect: false
        }
    }

    pub fn on_send(&mut self) {
        self.last_send_packet = Instant::now();
    }

    pub fn on_receive(&mut self) {
        self.last_received_packet = Instant::now();
    }

}

