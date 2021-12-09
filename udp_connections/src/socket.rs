use std::io::Result;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use crate::client::Client;
use crate::constants::MAX_PACKET_SIZE;
use crate::packets::Packet;
use crate::server::Server;

pub trait UdpSocketImpl {
    fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> Result<usize>;
    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)>;
    fn local_addr(&self) -> Result<SocketAddr>;
}

impl UdpSocketImpl for UdpSocket {
    fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> Result<usize> {
        self.send_to(buf, addr)
    }

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        self.recv_from(buf)
    }

    fn local_addr(&self) -> Result<SocketAddr> {
        self.local_addr()
    }
}

#[derive(Debug)]
pub struct PacketSocket<U: UdpSocketImpl> {
    socket: U,
    buffer: [u8; MAX_PACKET_SIZE],
    salt: String
}

pub type UdpClient = Client<UdpSocket>;
impl UdpClient {
    pub fn new(identifier: &str) -> Result<Self> {
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let address = SocketAddrV4::new(loopback, 0);
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;

        Ok(Self::from_socket(socket, identifier))
    }
}

pub type UdpServer = Server<UdpSocket>;
impl UdpServer {
    pub fn listen<A: ToSocketAddrs>(address: A, identifier: &str, max_clients: u16) -> Result<Self> {
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        Ok(Self::from_socket(socket, identifier, max_clients))
    }
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

    pub fn send_with<C: Connection>(&mut self, packet: Packet, connection: &mut C) -> Result<()> {
        connection.on_send();
        self.send_to(packet, connection.addrs())
    }

}

pub trait Connection {
    fn on_send(&mut self);
    fn on_receive(&mut self);
    fn addrs(&self) -> SocketAddr;
}


