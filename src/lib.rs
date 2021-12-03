mod protocol;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::io::{Error, ErrorKind, Result};
use std::time::{Duration, Instant};
use crate::protocol::Packet;

pub const MAX_PACKET_SIZE: usize = 1500;
pub const HEADER_SIZE: usize = 0;
pub const MAX_PAYLOAD_SIZE: usize = MAX_PACKET_SIZE - HEADER_SIZE;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Copy, Clone)]
pub enum DisconnectReason {
    Disconnected,
    TimedOut,
    ConnectionDenied
}

#[derive(Debug, Copy, Clone)]
pub enum ClientEvent<'a> {
    Connected(u16),
    Disconnected(DisconnectReason),
    Packet(&'a [u8])
}

#[derive(Debug, Copy, Clone)]
enum ClientState {
    Disconnected,
    Connecting(SocketAddr, Instant),
    Connected(SocketAddr, Instant),
    Disconnecting(SocketAddr)
}

#[derive(Debug)]
struct PacketSocket {
    socket: UdpSocket,
    buffer: [u8; MAX_PACKET_SIZE],
    salt: String
}

impl PacketSocket {

    fn bind<A: ToSocketAddrs>(addrs: A, identifier: &str) -> Result<Self> {
        let socket = UdpSocket::bind(addrs)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            buffer: [0; MAX_PACKET_SIZE],
            salt: identifier.to_string()
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    fn recv_from(&mut self) -> Result<(Result<Packet>, SocketAddr)> {
        let (size, src) = self.socket.recv_from(&mut self.buffer)?;
        Ok((Packet::from(&self.buffer[..size], self.salt.as_bytes()), src))
    }

    fn send_to<A: ToSocketAddrs>(&mut self, packet: Packet, addrs: A) -> Result<()> {
        let packet = packet.write(&mut self.buffer, self.salt.as_bytes())?;
        let i = self.socket.send_to(packet, addrs)?;
        assert_eq!(packet.len(), i);
        Ok(())
    }

}

#[derive(Debug)]
pub struct UdpClient {
    socket: PacketSocket,
    state: ClientState
}

impl UdpClient {

    pub fn new(identifier: &str) -> Result<Self> {
        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        let address = SocketAddrV4::new(loopback, 0);
        let socket = PacketSocket::bind(address, identifier)?;
        Ok(Self {
            socket,
            state: ClientState::Disconnected
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn remote_addr(&self) -> Option<SocketAddr> {
        match self.state {
            ClientState::Disconnected => None,
            ClientState::Connecting(addrs, _) => Some(addrs),
            ClientState::Connected(addrs, _) => Some(addrs),
            ClientState::Disconnecting(addrs) => Some(addrs),
        }
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
                for _ in 0..10 {
                    self.send_internal(Packet::Disconnect)?;
                }
                self.state = ClientState::Disconnecting(addrs);
                Ok(())
            },
            _ => Err(Error::new(ErrorKind::NotConnected, "not connected to a server!"))
        }
    }

    pub fn update(&mut self) -> Result<()> {
        match self.state {
            ClientState::Connecting(_, _) => self.send_internal(Packet::ConnectionRequest),
            _ => Ok(())
        }
    }

    pub fn next_event<'a>(&mut self, payload: &'a mut [u8]) -> Result<Option<ClientEvent<'a>>> {
        let now = Instant::now();

        match self.state {
            ClientState::Connecting(_, start) => if (now - start) > CONNECTION_TIMEOUT {
                self.state = ClientState::Disconnected;
                return Ok(Some(ClientEvent::Disconnected(DisconnectReason::TimedOut)))
            },
            ClientState::Connected(_, last) if (now - last) > CONNECTION_TIMEOUT => {
                self.state = ClientState::Disconnected;
                return Ok(Some(ClientEvent::Disconnected(DisconnectReason::TimedOut)))
            },
            ClientState::Disconnecting(_) => {
                self.state = ClientState::Disconnected;
                return Ok(Some(ClientEvent::Disconnected(DisconnectReason::Disconnected)))
            }
            _ => {}
        }

        match self.socket.recv_from() {
            Ok((packet, src)) => match self.state {
                ClientState::Connecting(remote, _) if remote == src => match packet{
                    Ok(Packet::ConnectionAccepted(id)) => {
                        self.state = ClientState::Connected(src, now);
                        Ok(Some(ClientEvent::Connected(id)))
                    },
                    Ok(Packet::ConnectionDenied) => {
                        self.state = ClientState::Disconnected;
                        Ok(Some(ClientEvent::Disconnected(DisconnectReason::ConnectionDenied)))
                    }
                    _ => self.next_event(payload)
                },
                ClientState::Connected(remote, _) if remote == src => match packet{
                    Ok(Packet::Payload(data)) => {
                        let result = &mut payload[..data.len()];
                        result.copy_from_slice(data);
                        self.state = ClientState::Connected(remote, now);
                        Ok(Some(ClientEvent::Packet(result)))
                    },
                    Ok(Packet::Disconnect) => {
                        self.state = ClientState::Disconnected;
                        Ok(Some(ClientEvent::Disconnected(DisconnectReason::Disconnected)))
                    },
                    _ => self.next_event(payload)
                }
                _ => self.next_event(payload)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock) => Ok(None),
            Err(e) => Err(e)
        }
    }

    fn send_internal(&mut self, packet: Packet) -> Result<()> {
        match self.remote_addr() {
            None => Err(Error::new(ErrorKind::NotConnected, "not connected to a server")),
            Some(addrs) => self.socket.send_to(packet, addrs)
        }
    }

    pub fn send(&mut self, payload: &[u8]) -> Result<()> {
        match self.state {
            ClientState::Connected(_, _) => self.send_internal(Packet::Payload(payload)),
            _ => Err(Error::new(ErrorKind::NotConnected, "not connected to a server"))
        }
    }

}

#[derive(Debug, Copy, Clone)]
pub enum ServerEvent<'a> {
    ClientConnected(u16),
    ClientDisconnected(u16, DisconnectReason),
    Packet(u16, &'a [u8])
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

    #[allow(dead_code)]
    fn iter_mut(&mut self)  -> impl Iterator<Item=&mut VirtualConnection> {
        self.0.iter_mut().filter_map(|c|c.as_mut())
    }

}


#[derive(Debug)]
pub struct UdpServer {
    socket: PacketSocket,
    clients: ConnectionManager
}

impl UdpServer {

    pub fn listen<A: ToSocketAddrs>(address: A, identifier: &str, max_clients: u16) -> Result<Self> {
        let socket = PacketSocket::bind(address, identifier)?;
        let clients = ConnectionManager::new(max_clients);
        Ok(Self {
            socket,
            clients
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn update(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn next_event<'a>(&mut self, payload: &'a mut [u8]) -> Result<Option<ServerEvent<'a>>> {
        let now = Instant::now();

        {
            let disconnect_client = self.clients.iter().filter_map(|v|match v {
                v if v.should_disconnect
                    => Some((v.id, DisconnectReason::Disconnected)),
                v if (now - v.last_received_packet) > CONNECTION_TIMEOUT
                    => Some((v.id, DisconnectReason::TimedOut)),
                _ => None
            }).next();
            if let Some((id, reason)) = disconnect_client{
                self.clients.disconnect(id);
                return Ok(Some(ServerEvent::ClientDisconnected(id, reason)))
            }
        }

        match self.socket.recv_from() {
            Ok((packet, src)) => match packet {
                Ok(Packet::ConnectionRequest) => match self.clients.find_by_addrs(src){
                    None => match self.clients.create_new_connection(src) {
                        None => {
                            self.socket.send_to(Packet::ConnectionDenied, src)?;
                            self.next_event(payload)
                        },
                        Some(conn) => {
                            self.socket.send_to(Packet::ConnectionAccepted(conn.id), conn.addrs)?;
                            Ok(Some(ServerEvent::ClientConnected(conn.id)))
                        }
                    },
                    Some(conn) => {
                        conn.last_received_packet = now;
                        let packet = Packet::ConnectionAccepted(conn.id);
                        self.socket.send_to(packet, conn.addrs)?;
                        self.next_event(payload)
                    }
                },
                Ok(Packet::Payload(data)) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        let result = &mut payload[..data.len()];
                        result.copy_from_slice(data);
                        conn.last_received_packet = now;
                        Ok(Some(ServerEvent::Packet(conn.id, result)))
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
        for conn in self.clients.iter() {
            self.socket.send_to(Packet::Payload(payload), conn.addrs)?;
        }
        Ok(())
    }

    pub fn send(&mut self, client_id: u16, payload: &[u8]) -> Result<()> {
        match self.clients.get(client_id) {
            Some(conn) => self.socket.send_to(Packet::Payload(payload), conn.addrs),
            None => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

    pub fn disconnect(&mut self, client_id: u16) -> Result<()> {
        match self.clients.get_mut(client_id) {
            Some(conn) => {
                for _ in 0..10 {
                    self.socket.send_to(Packet::Disconnect, conn.addrs)?
                }
                conn.should_disconnect = true;
                Ok(())
            },
            None => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

}

