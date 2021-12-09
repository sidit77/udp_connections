use std::io::{Error, ErrorKind, Result};
use std::net::{SocketAddr};
use std::time::Instant;
use crate::constants::{CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL};
use crate::packets::Packet;
use crate::socket::{Connection, PacketSocket, UdpSocketImpl};

#[derive(Debug, Copy, Clone)]
pub enum ServerDisconnectReason {
    Disconnected,
    TimedOut,
    ConnectionDenied
}

#[derive(Debug, Copy, Clone)]
pub enum ServerEvent<'a> {
    ClientConnected(u16),
    ClientDisconnected(u16, ServerDisconnectReason),
    Packet(u16, &'a [u8])
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VirtualConnection {
    pub(crate) addrs: SocketAddr,
    id: u16,
    pub(crate) last_received_packet: Instant,
    pub(crate) last_send_packet: Instant,
    should_disconnect: bool
}

impl VirtualConnection {
    pub(crate) fn new(addrs: SocketAddr, id: u16) -> Self {
        Self {
            addrs,
            id,
            last_received_packet: Instant::now(),
            last_send_packet: Instant::now(),
            should_disconnect: false
        }
    }

}

impl Connection for VirtualConnection {
    fn on_send(&mut self) {
        self.last_send_packet = Instant::now();
    }

    fn on_receive(&mut self) {
        self.last_received_packet = Instant::now();
    }

    fn addrs(&self) -> SocketAddr {
        self.addrs
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
pub struct Server<U: UdpSocketImpl> {
    socket: PacketSocket<U>,
    clients: ConnectionManager
}



impl<U: UdpSocketImpl> Server<U> {

    pub fn from_socket(socket: U, identifier: &str, max_clients: u16) -> Self {
        let socket = PacketSocket::new(socket, identifier);
        let clients = ConnectionManager::new(max_clients);
        Self {
            socket,
            clients
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn update(&mut self) -> Result<()> {
        let now = Instant::now();
        for conn in self.clients.iter_mut() {
            if (now - conn.last_send_packet) > KEEPALIVE_INTERVAL {
                self.socket.send_with(Packet::KeepAlive, conn)?
            }
        }
        Ok(())
    }

    pub fn next_event<'a>(&mut self, payload: &'a mut [u8]) -> Result<Option<ServerEvent<'a>>> {
        let now = Instant::now();

        {
            let disconnect_client = self.clients.iter().filter_map(|v|match v {
                v if v.should_disconnect
                => Some((v.id, ServerDisconnectReason::Disconnected)),
                v if (now - v.last_received_packet) > CONNECTION_TIMEOUT
                => Some((v.id, ServerDisconnectReason::TimedOut)),
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
                            self.socket.send_with(Packet::ConnectionAccepted(conn.id), conn)?;
                            Ok(Some(ServerEvent::ClientConnected(conn.id)))
                        }
                    },
                    Some(conn) => {
                        conn.on_receive();
                        self.socket.send_with(Packet::ConnectionAccepted(conn.id), conn)?;
                        self.next_event(payload)
                    }
                },
                Ok(Packet::Payload(data)) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        let result = &mut payload[..data.len()];
                        result.copy_from_slice(data);
                        conn.on_receive();
                        Ok(Some(ServerEvent::Packet(conn.id, result)))
                    },
                    None => self.next_event(payload)
                },
                Ok(Packet::KeepAlive) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        conn.on_receive();
                        self.next_event(payload)
                    },
                    None => self.next_event(payload)
                },
                Ok(Packet::Disconnect) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        let id = conn.id;
                        self.clients.disconnect(id);
                        Ok(Some(ServerEvent::ClientDisconnected(id, ServerDisconnectReason::Disconnected)))
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
            self.socket.send_with(Packet::Payload(payload), conn)?;
        }
        Ok(())
    }

    pub fn send(&mut self, client_id: u16, payload: &[u8]) -> Result<()> {
        match self.clients.get_mut(client_id) {
            Some(conn) => self.socket.send_with(Packet::Payload(payload), conn),
            None => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

    pub fn disconnect(&mut self, client_id: u16) -> Result<()> {
        match self.clients.get_mut(client_id) {
            Some(conn) => {
                for _ in 0..10 {
                    self.socket.send_with(Packet::Disconnect, conn)?
                }
                conn.should_disconnect = true;
                Ok(())
            },
            None => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

}

