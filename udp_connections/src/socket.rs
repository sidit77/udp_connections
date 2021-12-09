use std::io::Result;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use crate::client::Client;
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


