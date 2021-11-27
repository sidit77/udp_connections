mod protocol;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::io::{Error, ErrorKind, Result};
use crate::protocol::{ClientProtocol, ServerProtocol};

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

#[derive(Debug, Copy, Clone)]
enum ClientState {
    Disconnected,
    Connecting(SocketAddr),
    Connected(SocketAddr)
}

#[derive(Debug)]
pub struct UdpClient {
    socket: UdpSocket,
    state: ClientState
}

impl UdpClient {

    pub fn new() -> Result<Self> {
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let address = SocketAddrV4::new(loopback, 0);
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            state: ClientState::Disconnected
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn connect<A: ToSocketAddrs>(&mut self, address: A) -> Result<()> {
        match address.to_socket_addrs()?.next() {
            Some(addr) => {
               self.state = ClientState::Connecting(addr);
                Ok(())
            },
            None => Err(Error::new(ErrorKind::InvalidInput, "no addresses to connect to")),
        }
    }

    pub fn disconnect(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn next_event(&mut self, payload: &mut [u8]) -> Result<Option<ClientEvent>> {
        let acceptable = |src| match self.state {
            ClientState::Disconnected => false,
            ClientState::Connecting(addr) => src == addr,
            ClientState::Connected(addr) => src == addr,
        };

        let mut buffer = [0u8; MAX_PACKET_SIZE];

        match self.state {
            ClientState::Connecting(addr) => {
                self.socket.send_to(
                    ClientProtocol::ConnectionRequest.write(&mut buffer)?,
                    addr)?;
            }
            _ => {}
        }

        match self.socket.recv_from(&mut buffer) {
            Ok((size, src)) => match acceptable(src) {
                true => match self.state {
                    ClientState::Connecting(_) => match ServerProtocol::from(&buffer[..size]){
                        Ok(ServerProtocol::ConnectionAccepted) => {
                            self.state = ClientState::Connected(src);
                            Ok(Some(ClientEvent::Connected))
                        },
                        Ok(ServerProtocol::ConnectionDenied) => {
                            self.state = ClientState::Disconnected;
                            Ok(Some(ClientEvent::Disconnected(DisconnectReason::ConnectionDenied)))
                        }
                        _ => self.next_event(payload)
                    }
                    ClientState::Connected(_) => match ServerProtocol::from(&buffer[..size]){
                        Ok(ServerProtocol::Payload(data)) => {
                            payload[..data.len()].copy_from_slice(data);
                            Ok(Some(ClientEvent::Packet(data.len())))
                        },
                        _ => self.next_event(payload)
                    },
                    ClientState::Disconnected => self.next_event(payload)
                },
                false => self.next_event(payload)
            },
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock) => Ok(None),
            Err(e) => Err(e)
        }
    }

    pub fn send(&mut self, _payload: &mut [u8]) -> Result<()> {
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
        let mut buffer = [0u8; MAX_PACKET_SIZE];

        match self.socket.recv_from(&mut buffer) {
            Ok((size, src)) => match ClientProtocol::from(&buffer[..size]) {
                Ok(ClientProtocol::ConnectionRequest) => match self.get_client_id(src){
                    None => match self.get_free_client_id() {
                        None => {
                            self.socket.send_to(ServerProtocol::ConnectionDenied.write(&mut buffer)?, src)?;
                            self.next_event(payload)
                        },
                        Some(id) => {
                            self.clients[id as usize] = Some(src);
                            self.socket.send_to(ServerProtocol::ConnectionAccepted.write(&mut buffer)?, src)?;
                            Ok(Some(ServerEvent::ClientConnected(id)))
                        }
                    },
                    Some(id) => {
                        self.socket.send_to(ServerProtocol::ConnectionAccepted.write(&mut buffer)?, src)?;
                        self.next_event(payload)
                    }
                },
                Ok(ClientProtocol::Payload(data)) => match self.get_client_id(src) {
                    Some(id) => {
                        payload[..data.len()].copy_from_slice(data);
                        Ok(Some(ServerEvent::Packet(id, data.len())))
                    },
                    None => self.next_event(payload)
                },
                _ => self.next_event(payload)
            },
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock) => Ok(None),
            Err(e) => Err(e)
        }
    }

    fn get_client_id(&self, addr: SocketAddr) -> Option<u64> {
        self.clients
            .iter()
            .enumerate()
            .find_map(|(i, c) | c
                .filter(|a| *a == addr)
                .map(|_| i as u64))
    }

    fn get_free_client_id(&self) -> Option<u64> {
        self.clients
            .iter()
            .enumerate()
            .find_map(|(i, c) | match c {
                None => Some(i as u64),
                Some(_) => None
            })
    }

    pub fn send(&mut self, _client_id: u64, _payload: &mut [u8]) -> Result<()> {
        Ok(())
    }

    pub fn disconnect(&mut self, _client_id: u64) -> Result<()> {
        Ok(())
    }

}

