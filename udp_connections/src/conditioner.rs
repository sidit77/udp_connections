use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::io::Result;
use crate::client::Client;
use crate::server::Server;
use crate::socket::UdpSocketImpl;

#[derive(Debug, Copy, Clone)]
pub struct NetworkOptions {
    pub packet_loss: f32
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            packet_loss: 0.0
        }
    }
}

pub struct ConditionedUdpSocket {
    socket: UdpSocket,
    options: NetworkOptions
}

impl ConditionedUdpSocket {

    pub fn from_socket(socket: UdpSocket, options: NetworkOptions) -> Self {
        Self {
            socket,
            options
        }
    }

}

impl UdpSocketImpl for ConditionedUdpSocket {
    fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> std::io::Result<usize> {
        self.socket.send_to(buf, addr)
    }

    fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        match self.socket.recv_from(buf) {
            Ok(result) => if fastrand::f32() < self.options.packet_loss {
                self.recv_from(buf)
            } else {
                Ok(result)
            }
            Err(e) => Err(e)
        }
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}


pub type ConditionedUdpClient = Client<ConditionedUdpSocket>;
pub type ConditionedUdpServer = Server<ConditionedUdpSocket>;

impl ConditionedUdpClient {
    pub fn new(identifier: &str, options: NetworkOptions) -> Result<Self> {
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let address = SocketAddrV4::new(loopback, 0);
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;

        Ok(Self::from_socket(ConditionedUdpSocket::from_socket(socket, options), identifier))
    }
}

impl ConditionedUdpServer {
    pub fn listen<A: ToSocketAddrs>(address: A, identifier: &str, max_clients: u16, options: NetworkOptions) -> Result<Self> {
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        Ok(Self::from_socket(ConditionedUdpSocket::from_socket(socket, options), identifier, max_clients))
    }
}