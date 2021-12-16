use std::fmt::Debug;
use std::io::Result;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

pub trait Transport : Debug {
    fn send_to(&self, buf: &[u8], addr: SocketAddr) -> Result<usize>;
    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)>;
    fn local_addr(&self) -> Result<SocketAddr>;
}

impl Transport for UdpSocket {
    fn send_to(&self, buf: &[u8], addr: SocketAddr) -> Result<usize> {
        self.send_to(buf, addr)
    }

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        self.recv_from(buf)
    }

    fn local_addr(&self) -> Result<SocketAddr> {
        self.local_addr()
    }
}

const ANY_PORT: u16 = 0;

pub struct Endpoint;

impl Endpoint {

    pub fn remote_port(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)
    }

    pub fn remote_any() -> SocketAddr {
        Self::remote_port(ANY_PORT)
    }

    pub fn local_port(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    pub fn local_any() -> SocketAddr {
        Self::local_port(ANY_PORT)
    }

}