mod protocol;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::io::{Error, ErrorKind, Result};
use std::time::{Duration, Instant};
use crate::protocol::Packet;

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
    Connected(SocketAddr, Instant),
    Disconnecting(SocketAddr)
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
        match self.state {
            ClientState::Connected(addrs, _) => {
                self.state = ClientState::Disconnecting(addrs);
                Ok(())
            },
            _ => Err(Error::new(ErrorKind::NotConnected, "not connected to a server!"))
        }
    }

    pub fn next_event(&mut self, payload: &mut [u8]) -> Result<Option<ClientEvent>> {
        let mut buffer = [0u8; MAX_PACKET_SIZE];
        let now = Instant::now();

        match self.state {
            ClientState::Connecting(_, start) => {
                if (now - start) > CONNECTION_TIMEOUT {
                    self.state = ClientState::Disconnected;
                    return Ok(Some(ClientEvent::Disconnected(DisconnectReason::TimedOut)))
                } else {
                    self.send_internal(Packet::ConnectionRequest)?;
                }
            },
            ClientState::Connected(_, last) => {
                if (now - last) > CONNECTION_TIMEOUT {
                    self.state = ClientState::Disconnected;
                    return Ok(Some(ClientEvent::Disconnected(DisconnectReason::TimedOut)))
                }
            },
            ClientState::Disconnecting(_) => {
                for _ in 0..10 {
                    self.send_internal(Packet::Disconnect)?;
                }
                self.state = ClientState::Disconnected;
                return Ok(Some(ClientEvent::Disconnected(DisconnectReason::Disconnected)))
            }
            _ => {}
        }

        let acceptable = |src| match self.state {
            ClientState::Disconnected => false,
            ClientState::Disconnecting(_) => false,
            ClientState::Connecting(addr, _) => src == addr,
            ClientState::Connected(addr, _) => src == addr,
        };

        match self.socket.recv_from(&mut buffer) {
            Ok((size, src)) => match acceptable(src) {
                true => match &mut self.state {
                    ClientState::Connecting(_, _) => match Packet::from(&buffer[..size]){
                        Ok(Packet::ConnectionAccepted(id)) => {
                            self.state = ClientState::Connected(src, now);
                            Ok(Some(ClientEvent::Connected(id)))
                        },
                        Ok(Packet::ConnectionDenied) => {
                            self.state = ClientState::Disconnected;
                            Ok(Some(ClientEvent::Disconnected(DisconnectReason::ConnectionDenied)))
                        }
                        _ => self.next_event(payload)
                    }
                    ClientState::Connected(_, last_packet) => match Packet::from(&buffer[..size]){
                        Ok(Packet::Payload(data)) => {
                            payload[..data.len()].copy_from_slice(data);
                            *last_packet = now;
                            Ok(Some(ClientEvent::Packet(data.len())))
                        },
                        Ok(Packet::Disconnect) => {
                            self.state = ClientState::Disconnected;
                            Ok(Some(ClientEvent::Disconnected(DisconnectReason::Disconnected)))
                        },
                        _ => self.next_event(payload)
                    },
                    _ => self.next_event(payload)
                },
                false => self.next_event(payload)
            },
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock) => Ok(None),
            Err(e) => Err(e)
        }
    }

    fn send_internal(&mut self, packet: Packet) -> Result<()> {
        let mut buffer = [0u8; MAX_PACKET_SIZE];
        let packet = packet.write(&mut buffer)?;
        let _ = match self.state {
            ClientState::Disconnected => Err(Error::new(ErrorKind::NotConnected, "not connected to a server")),
            ClientState::Connecting(addrs, _) =>  self.socket.send_to(packet, addrs),
            ClientState::Connected(addrs, _) => self.socket.send_to(packet, addrs),
            ClientState::Disconnecting(addrs) => self.socket.send_to(packet, addrs),
        }?;
        Ok(())
    }

    pub fn send(&mut self, payload: &[u8]) -> Result<()> {
        match self.state {
            ClientState::Connected(_, _) => self.send_internal(Packet::Payload(payload)),
            _ => Err(Error::new(ErrorKind::NotConnected, "not connected to a server"))
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
    last_received_packet: Instant,
    should_disconnect: bool
}

impl VirtualConnection {
    fn new(addrs: SocketAddr, id: u16) -> Self {
        Self {
            addrs,
            id,
            last_received_packet: Instant::now(),
            should_disconnect: false
        }
    }

    fn send(&mut self, socket: &UdpSocket, packet: Packet) -> Result<()> {
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

    #[allow(dead_code)]
    fn get(&self, id: u16) -> Option<&VirtualConnection> {
        self.0.get(id as usize).map(|c|c.as_ref()).flatten()
    }

    fn get_mut(&mut self, id: u16) -> Option<&mut VirtualConnection> {
        self.0.get_mut(id as usize).map(|c|c.as_mut()).flatten()
    }

    fn find_by_addrs(&mut self, addrs: SocketAddr) -> Option<&mut VirtualConnection> {
        self.0
            .iter_mut()
            .filter_map(|c|c.as_mut())
            .find(|c|c.addrs == addrs)
    }

    fn create_new_connection(&mut self, addrs: SocketAddr) -> Option<&mut VirtualConnection> {
        self.0
            .iter_mut()
            .enumerate()
            .find(|(_, c)|c.is_none())
            .map(|(id, c)| {
                *c = Some(VirtualConnection::new(addrs, id as u16));
                c.as_mut().unwrap()
            })
    }

    fn disconnect(&mut self, id: u16) -> bool {
        match self.0.get_mut(id as usize) {
            None => false,
            Some(slot) => {
                *slot = None;
                true
            }
        }
    }

    fn iter(&self) -> impl Iterator<Item=&VirtualConnection> {
        self.0.iter().filter_map(|c|c.as_ref())
    }

    fn iter_mut(&mut self)  -> impl Iterator<Item=&mut VirtualConnection> {
        self.0.iter_mut().filter_map(|c|c.as_mut())
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

        {
            let disconnect_client =
                self.clients.iter_mut().find(|c|c.should_disconnect);
            if let Some(conn) = disconnect_client{
                let id = conn.id;
                for _ in 0..10 { conn.send(&self.socket, Packet::Disconnect)? }
                self.clients.disconnect(id);
                return Ok(Some(ServerEvent::ClientDisconnected(id, DisconnectReason::Disconnected)))
            }
            let timeout_client = self.clients.iter()
                .find(|c|(now - c.last_received_packet) > CONNECTION_TIMEOUT);
            if let Some(conn) = timeout_client {
                let id = conn.id;
                self.clients.disconnect(id);
                return Ok(Some(ServerEvent::ClientDisconnected(id, DisconnectReason::TimedOut)))
            }
        }

        match self.socket.recv_from(&mut buffer) {
            Ok((size, src)) => match Packet::from(&buffer[..size]) {
                Ok(Packet::ConnectionRequest) => match self.clients.find_by_addrs(src){
                    None => match self.clients.create_new_connection(src) {
                        None => {
                            let mut tmp = VirtualConnection::new(src, 0);
                            tmp.send(&self.socket, Packet::ConnectionDenied)?;
                            self.next_event(payload)
                        },
                        Some(conn) => {
                            conn.send(&self.socket, Packet::ConnectionAccepted(conn.id))?;
                            Ok(Some(ServerEvent::ClientConnected(conn.id)))
                        }
                    },
                    Some(conn) => {
                        conn.last_received_packet = now;
                        let packet = Packet::ConnectionAccepted(conn.id);
                        conn.send(&self.socket, packet)?;
                        self.next_event(payload)
                    }
                },
                Ok(Packet::Payload(data)) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        payload[..data.len()].copy_from_slice(data);
                        conn.last_received_packet = now;
                        Ok(Some(ServerEvent::Packet(conn.id, data.len())))
                    },
                    None => self.next_event(payload)
                },
                Ok(Packet::Disconnect) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        let id = conn.id;
                        self.clients.disconnect(id);
                        Ok(Some(ServerEvent::ClientDisconnected(id, DisconnectReason::Disconnected)))
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
        self.clients.iter().map(|v|v.id)
    }

    pub fn broadcast(&mut self, payload: &[u8]) -> Result<()> {
        for conn in self.clients.iter_mut() {
            conn.send(&self.socket, Packet::Payload(payload))?;
        }
        Ok(())
    }

    pub fn send(&mut self, client_id: u16, payload: &[u8]) -> Result<()> {
        match self.clients.get_mut(client_id) {
            Some(conn) => conn.send(&self.socket, Packet::Payload(payload)),
            None => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

    pub fn disconnect(&mut self, client_id: u16) -> Result<()> {
        match self.clients.get_mut(client_id) {
            Some(conn) => {
                conn.should_disconnect = true;
                Ok(())
            },
            None => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

}

