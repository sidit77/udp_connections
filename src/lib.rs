use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::io::Result;

pub const MAX_PACKET_SIZE: usize = 1500;
pub const HEADER_SIZE: usize = 0;
pub const MAX_PAYLOAD_SIZE: usize = MAX_PACKET_SIZE - HEADER_SIZE;

#[derive(Debug, Copy, Clone)]
pub enum DisconnectReason {
    Disconnected,
    TimedOut,
    ConnectionDenied
}

#[derive(Debug, Copy, Clone)]
pub enum ClientEvent {
    Connected,
    Disconnected(DisconnectReason),
    Packet(usize)
}

#[derive(Debug)]
pub struct UdpClient {
    socket: UdpSocket,
    server: Option<SocketAddr>
}

impl UdpClient {

    pub fn new() -> Result<Self> {
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let address = SocketAddrV4::new(loopback, 0);
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            server: None
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn connect<A: ToSocketAddrs>(&mut self, address: A) -> Result<()> {
        Ok(())
    }

    pub fn disconnect(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn next_event(&mut self, payload: &mut [u8]) -> Result<Option<ClientEvent>> {
        Ok(None)
    }

    pub fn send(&mut self, payload: &mut [u8]) -> Result<()> {
        Ok(())
    }

}

#[derive(Debug, Copy, Clone)]
pub enum ServerEvent {
    ClientConnected(u64),
    ClientDisconnected(u64, DisconnectReason),
    Packet(u64, usize)
}

#[derive(Debug)]
pub struct UdpServer {
    socket: UdpSocket,
    clients: Box<[Option<SocketAddr>]>
}

impl UdpServer {

    pub fn listen<A: ToSocketAddrs>(address: A, max_clients: usize) -> Result<Self> {
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        let clients = vec![None; max_clients].into_boxed_slice();
        Ok(Self {
            socket,
            clients
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn next_event(&mut self, payload: &mut [u8]) -> Result<Option<ServerEvent>> {
        Ok(None)
    }

    pub fn send(&mut self, client_id: u64, payload: &mut [u8]) -> Result<()> {
        Ok(())
    }

    pub fn disconnect(&mut self, client_id: u64) -> Result<()> {
        Ok(())
    }

}
