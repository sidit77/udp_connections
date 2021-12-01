mod protocol;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::io::{Error, ErrorKind, Result};
use std::slice::{Iter, IterMut};
use std::time::{Duration, Instant};
use crate::protocol::{ClientProtocol, ServerProtocol};

pub const MAX_PACKET_SIZE: usize = 1500;
pub const HEADER_SIZE: usize = 0;
pub const MAX_PAYLOAD_SIZE: usize = MAX_PACKET_SIZE - HEADER_SIZE;

pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Copy, Clone)]
pub enum DisconnectReason {
    Disconnected,
    TimedOut,
    ConnectionDenied
}

#[derive(Debug, Copy, Clone)]
pub enum ClientEvent {
    Connected(u16),
    Disconnected(DisconnectReason),
    Packet(usize)
}

#[derive(Debug, Copy, Clone)]
enum ClientState {
    Disconnected,
    Connecting(SocketAddr, Instant),
    Connected(SocketAddr, Instant)
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

    pub fn is_connected(&self) -> bool {
        matches!(self.state, ClientState::Connected(_, _))
    }

    pub fn connect<A: ToSocketAddrs>(&mut self, address: A) -> Result<()> {
        match address.to_socket_addrs()?.next() {
            Some(addr) => {
               self.state = ClientState::Connecting(addr, Instant::now());
                Ok(())
            },
            None => Err(Error::new(ErrorKind::InvalidInput, "no addresses to connect to")),
        }
    }

    pub fn disconnect(&mut self) -> Result<()> {
        unimplemented!()
    }

    pub fn next_event(&mut self, payload: &mut [u8]) -> Result<Option<ClientEvent>> {
        let acceptable = |src| match self.state {
            ClientState::Disconnected => false,
            ClientState::Connecting(addr, _) => src == addr,
            ClientState::Connected(addr, _) => src == addr,
        };

        let mut buffer = [0u8; MAX_PACKET_SIZE];
        let now = Instant::now();

        match self.state {
            ClientState::Connecting(addr, start) => {
                if (now - start) > CONNECTION_TIMEOUT {
                    self.state = ClientState::Disconnected;
                    return Ok(Some(ClientEvent::Disconnected(DisconnectReason::TimedOut)))
                } else {
                    self.socket.send_to(
                        ClientProtocol::ConnectionRequest.write(&mut buffer)?,
                        addr)?;
                }
            },
            ClientState::Connected(_, last) => {
                if (now - last) > CONNECTION_TIMEOUT {
                    self.state = ClientState::Disconnected;
                    return Ok(Some(ClientEvent::Disconnected(DisconnectReason::TimedOut)))
                }
            }
            _ => {}
        }

        match self.socket.recv_from(&mut buffer) {
            Ok((size, src)) => match acceptable(src) {
                true => match &mut self.state {
                    ClientState::Connecting(_, _) => match ServerProtocol::from(&buffer[..size]){
                        Ok(ServerProtocol::ConnectionAccepted(id)) => {
                            self.state = ClientState::Connected(src, now);
                            Ok(Some(ClientEvent::Connected(id)))
                        },
                        Ok(ServerProtocol::ConnectionDenied) => {
                            self.state = ClientState::Disconnected;
                            Ok(Some(ClientEvent::Disconnected(DisconnectReason::ConnectionDenied)))
                        }
                        _ => self.next_event(payload)
                    }
                    ClientState::Connected(_, last_packet) => match ServerProtocol::from(&buffer[..size]){
                        Ok(ServerProtocol::Payload(data)) => {
                            payload[..data.len()].copy_from_slice(data);
                            *last_packet = now;
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

    pub fn send(&mut self, payload: &[u8]) -> Result<()> {
        match self.state {
            ClientState::Connected(addr, _) => {
                let mut buffer = [0u8; MAX_PACKET_SIZE];
                self.socket.send_to(ClientProtocol::Payload(payload).write(&mut buffer)?, addr)?;
                Ok(())
            },
            _ => Err(Error::new(ErrorKind::NotConnected, "not connect to a server"))
        }
    }

}

#[derive(Debug, Copy, Clone)]
pub enum ServerEvent {
    ClientConnected(u16),
    ClientDisconnected(u16, DisconnectReason),
    Packet(u16, usize)
}

#[derive(Debug, Clone)]
struct VirtualConnection {
    addrs: SocketAddr,
    id: u16,
    last_received_packet: Instant
}

impl VirtualConnection {
    fn new(addrs: SocketAddr, id: u16) -> Self {
        Self {
            addrs,
            id,
            last_received_packet: Instant::now()
        }
    }

    fn send(&self, socket: &UdpSocket, packet: ServerProtocol) -> Result<()> {
        let mut buffer = [0u8; MAX_PACKET_SIZE];
        socket.send_to(packet.write(&mut buffer)?, self.addrs)?;
        Ok(())
    }
}

#[derive(Debug)]
struct ConnectionManager(Box<[Option<VirtualConnection>]>);

impl ConnectionManager {

    fn new(max_clients: u16) -> Self{
        Self {
            0:  vec![None; max_clients as usize].into_boxed_slice()
        }
    }

    fn get(&self, id: u16) -> Option<&VirtualConnection> {
        self.0.get(id as usize).unwrap_or(&None).as_ref()
    }

    fn find_by_addrs(&mut self, addrs: SocketAddr) -> Option<&mut VirtualConnection> {
        self.iter_mut()
            .filter_map(|c|c.as_mut())
            .find(|c|c.addrs == addrs)
    }

    fn create_new_connection(&mut self, addrs: SocketAddr) -> Option<&mut VirtualConnection> {
        self.iter_mut()
            .enumerate()
            .find(|(_, c)|c.is_none())
            .map(|(id, c)| {
                *c = Some(VirtualConnection::new(addrs, id as u16));
                c.as_mut().unwrap()
            })
    }

    fn iter(&self) -> Iter<'_, Option<VirtualConnection>> {
        self.0.iter()
    }

    fn iter_mut(&mut self) -> IterMut<'_, Option<VirtualConnection>> {
        self.0.iter_mut()
    }

}


#[derive(Debug)]
pub struct UdpServer {
    socket: UdpSocket,
    clients: ConnectionManager
}

impl UdpServer {

    pub fn listen<A: ToSocketAddrs>(address: A, max_clients: u16) -> Result<Self> {
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        let clients = ConnectionManager::new(max_clients);
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
        let now = Instant::now();

        for client in self.clients.iter_mut() {
            if let Some(conn) = client {
                if (now - conn.last_received_packet) > CONNECTION_TIMEOUT {
                    let id = conn.id;
                    *client = None;
                    return Ok(Some(ServerEvent::ClientDisconnected(id, DisconnectReason::TimedOut)))
                }
            }
        }

        match self.socket.recv_from(&mut buffer) {
            Ok((size, src)) => match ClientProtocol::from(&buffer[..size]) {
                Ok(ClientProtocol::ConnectionRequest) => match self.clients.find_by_addrs(src){
                    None => match self.clients.create_new_connection(src) {
                        None => {
                            let tmp = VirtualConnection::new(src, 0);
                            tmp.send(&self.socket, ServerProtocol::ConnectionDenied)?;
                            self.next_event(payload)
                        },
                        Some(conn) => {
                            conn.send(&self.socket, ServerProtocol::ConnectionAccepted(conn.id))?;
                            Ok(Some(ServerEvent::ClientConnected(conn.id)))
                        }
                    },
                    Some(conn) => {
                        conn.last_received_packet = now;
                        let packet = ServerProtocol::ConnectionAccepted(conn.id);
                        conn.send(&self.socket, packet)?;
                        self.next_event(payload)
                    }
                },
                Ok(ClientProtocol::Payload(data)) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        payload[..data.len()].copy_from_slice(data);
                        conn.last_received_packet = now;
                        Ok(Some(ServerEvent::Packet(conn.id, data.len())))
                    },
                    None => self.next_event(payload)
                },
                _ => self.next_event(payload)
            },
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock) => Ok(None),
            Err(e) => Err(e)
        }
    }

    pub fn connected_clients(&self) -> impl Iterator<Item=u16> +'_ {
        self.clients.iter().filter_map(|i|i.as_ref().map(|j|j.id))
    }

    pub fn broadcast(&mut self, payload: &[u8]) -> Result<()> {
        for client in self.clients.iter() {
            if let Some(conn) = client {
                conn.send(&self.socket, ServerProtocol::Payload(payload))?;
            }
        }
        Ok(())
    }

    pub fn send(&mut self, client_id: u16, payload: &[u8]) -> Result<()> {
        match self.clients.get(client_id) {
            Some(conn) => conn.send(&self.socket, ServerProtocol::Payload(payload)),
            None => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

    pub fn disconnect(&mut self, _client_id: u16) -> Result<()> {
        unimplemented!()
    }

}

