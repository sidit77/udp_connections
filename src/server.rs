use std::collections::VecDeque;
use std::io::{Error, ErrorKind, Result};
use std::net::{SocketAddr};
use std::time::Instant;
use crate::connection::{PacketSocket, VirtualConnection};
use crate::constants::{CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL};
use crate::packets::Packet;
use crate::sequencing::SequenceNumber;
use crate::socket::UdpSocketImpl;

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
    PacketReceived(u16, SequenceNumber, &'a [u8]),
    PacketAcknowledged(u16, SequenceNumber)
}

#[derive(Debug, Clone)]
enum ClientState {
    Disconnected,
    Connected(VirtualConnection),
    Disconnecting
}

impl Default for ClientState {
    fn default() -> Self {
        ClientState::Disconnected
    }
}

impl ClientState {

    fn get_connection(&self) -> Option<&VirtualConnection> {
        match self {
            ClientState::Connected(vc) => Some(vc),
            _ => None
        }
    }

    fn get_connection_mut(&mut self) -> Option<&mut VirtualConnection> {
        match self {
            ClientState::Connected(vc) => Some(vc),
            _ => None
        }
    }
}

#[derive(Debug)]
struct ConnectionManager(Box<[ClientState]>);

impl ConnectionManager {

    fn new(max_clients: u16) -> Self{
        Self {
            0:  vec![ClientState::Disconnected; max_clients as usize].into_boxed_slice()
        }
    }

    fn get(&self, id: u16) -> &ClientState {
        &self.0[id as usize]
    }

    fn get_mut(&mut self, id: u16) -> &mut ClientState {
        &mut self.0[id as usize]
    }

    fn find_by_addrs(&mut self, addrs: SocketAddr) -> Option<&mut VirtualConnection> {
        self.connections_mut().find(|c| c.addrs() == addrs)
    }

    fn create_new_connection(&mut self, addrs: SocketAddr) -> Option<&mut VirtualConnection> {
        self.slots_mut().find_map(|(id, state)| match state {
            ClientState::Disconnected => {
                *state = ClientState::Connected(VirtualConnection::new(addrs, id));
                state.get_connection_mut()
            },
            _ => None
        })
    }

    fn connections(&self) -> impl Iterator<Item=&VirtualConnection> {
        self.0.iter().filter_map(|c|c.get_connection())
    }

    fn connections_mut(&mut self) -> impl Iterator<Item=&mut VirtualConnection> {
        self.0.iter_mut().filter_map(|c|c.get_connection_mut())
    }

    fn slots_mut(&mut self) -> impl Iterator<Item=(u16, &mut ClientState)> {
        self.0.iter_mut().enumerate().map(|(id, state)|(id as u16, state))
    }

}


#[derive(Debug)]
pub struct Server<U: UdpSocketImpl> {
    socket: PacketSocket<U>,
    clients: ConnectionManager,
    ack_queue: VecDeque<(u16, SequenceNumber)>
}


impl<U: UdpSocketImpl> Server<U> {

    pub fn from_socket(socket: U, identifier: &str, max_clients: u16) -> Self {
        let socket = PacketSocket::new(socket, identifier);
        let clients = ConnectionManager::new(max_clients);
        Self {
            socket,
            clients,
            ack_queue: VecDeque::new()
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn update(&mut self) -> Result<()> {
        let now = Instant::now();
        for conn in self.clients.connections_mut() {
            if (now - conn.last_send_packet) > KEEPALIVE_INTERVAL {
                self.socket.send_keepalive(conn)?
            }
        }
        Ok(())
    }

    pub fn next_event<'a>(&mut self, payload: &'a mut [u8]) -> Result<Option<ServerEvent<'a>>> {
        let now = Instant::now();

        if let Some((client, seq)) = self.ack_queue.pop_front() {
            return Ok(Some(ServerEvent::PacketAcknowledged(client, seq)))
        }

        {
            let disconnect_client = self.clients.slots_mut().filter_map(|(id, state)|match state {
                ClientState::Disconnecting => Some((id, ServerDisconnectReason::Disconnected)),
                ClientState::Connected(v) if (now - v.last_received_packet) > CONNECTION_TIMEOUT
                                              => Some((v.id(), ServerDisconnectReason::TimedOut)),
                _ => None
            }).next();
            if let Some((id, reason)) = disconnect_client{
                *self.clients.get_mut(id) = ClientState::Disconnected;
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
                            self.socket.send_with(Packet::ConnectionAccepted(conn.id()), conn)?;
                            Ok(Some(ServerEvent::ClientConnected(conn.id())))
                        }
                    },
                    Some(conn) => {
                        conn.on_receive();
                        self.socket.send_with(Packet::ConnectionAccepted(conn.id()), conn)?;
                        self.next_event(payload)
                    }
                },
                Ok(Packet::Payload(seq, ack, data)) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        if conn.handle_seq(seq) {
                            let id = conn.id();
                            conn.handle_ack(ack, |i|self.ack_queue.push_back((id, i)));
                            conn.on_receive();
                            let result = &mut payload[..data.len()];
                            result.copy_from_slice(data);
                            Ok(Some(ServerEvent::PacketReceived(id, seq, result)))
                        } else {
                            self.next_event(payload)
                        }
                    },
                    None => self.next_event(payload)
                },
                Ok(Packet::KeepAlive(ack)) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        let id = conn.id();
                        conn.on_receive();
                        conn.handle_ack(ack, |i|self.ack_queue.push_back((id, i)));
                        self.next_event(payload)
                    },
                    None => self.next_event(payload)
                },
                Ok(Packet::Disconnect) => match self.clients.find_by_addrs(src) {
                    Some(conn) => {
                        let id = conn.id();
                        *self.clients.get_mut(id) = ClientState::Disconnected;
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
        self.clients.connections().map(|v|v.id())
    }

    pub fn broadcast(&mut self, payload: &[u8]) -> Result<()> {
        for conn in self.clients.connections_mut() {
            self.socket.send_payload(payload, conn)?;
        }
        Ok(())
    }

    pub fn send(&mut self, client_id: u16, payload: &[u8]) -> Result<SequenceNumber> {
        match self.clients.get_mut(client_id) {
            ClientState::Connected(conn) => self.socket.send_payload(payload, conn),
            _ => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

    pub fn disconnect(&mut self, client_id: u16) -> Result<()> {
        let state = self.clients.get_mut(client_id);
        match state {
            ClientState::Connected(conn) => {
                for _ in 0..10 {
                    self.socket.send_with(Packet::Disconnect, conn)?
                }
                *state = ClientState::Disconnecting;
                Ok(())
            },
            _ => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

    pub fn next_sequence_number(&self, client_id: u16) -> Result<SequenceNumber> {
        match self.clients.get(client_id) {
            ClientState::Connected(conn) => Ok(conn.peek_next_sequence_number()),
            _ => Err(Error::new(ErrorKind::NotConnected, "client not connected"))
        }
    }

}

