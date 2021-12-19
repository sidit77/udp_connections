use std::collections::VecDeque;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::io::ErrorKind;
use crate::connection::{PacketSocket, VirtualConnection};
use crate::constants::{CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL};
use crate::error::{ConnectionError, IOResult};
use crate::packets::Packet;
use crate::sequencing::SequenceNumber;
use crate::socket::Transport;

#[derive(Debug, Clone)]
pub enum ServerDisconnectReason {
    Disconnected,
    TimedOut,
    SocketError(ErrorKind)
}

#[derive(Debug)]
pub enum ServerEvent<'a> {
    ClientConnected(u16),
    ClientDisconnected(u16, ServerDisconnectReason),
    PacketReceived(u16, bool, &'a [u8]),
    PacketAcknowledged(u16, SequenceNumber)
}

#[derive(Debug, Clone)]
enum ClientState {
    Disconnected,
    Connected(VirtualConnection),
    Disconnecting(ServerDisconnectReason)
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

    fn get(&self, id: u16) -> Option<&ClientState> {
        self.0.get(id as usize)
    }

    fn get_mut(&mut self, id: u16) -> Option<&mut ClientState> {
        self.0.get_mut(id as usize)
    }

    fn set(&mut self, id: u16, new_state: ClientState) {
        *self.get_mut(id).unwrap() = new_state;
    }

    fn get_connection(&self, client_id: u16) -> Result<&VirtualConnection, ConnectionError> {
        match self.get(client_id) {
            Some(client) => match client {
                ClientState::Connected(connection) => Ok(connection),
                ClientState::Disconnected => Err(ConnectionError::Disconnected),
                _ => Err(ConnectionError::ConnectionNotReady)
            }
            None => Err(ConnectionError::Disconnected)
        }
    }

    fn get_connection_mut(&mut self, client_id: u16) -> Result<&mut VirtualConnection, ConnectionError> {
        match self.get_mut(client_id) {
            Some(client) => match client {
                ClientState::Connected(connection) => Ok(connection),
                ClientState::Disconnected => Err(ConnectionError::Disconnected),
                _ => Err(ConnectionError::ConnectionNotReady)
            }
            None => Err(ConnectionError::Disconnected)
        }
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
pub struct Server {
    socket: PacketSocket,
    clients: ConnectionManager,
    ack_queue: VecDeque<(u16, SequenceNumber)>
}


impl Server {

    pub fn new<T: Transport + 'static>(socket: T, identifier: &str, max_clients: u16) -> Self {
        let socket = PacketSocket::new(socket, identifier);
        let clients = ConnectionManager::new(max_clients);
        Self {
            socket,
            clients,
            ack_queue: VecDeque::new()
        }
    }

    pub fn local_addr(&self) -> IOResult<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn update(&mut self) {
        for (_, client) in self.clients.slots_mut() {
            if let Some(connection) = client.get_connection_mut() {
                if connection.last_packet_send() > KEEPALIVE_INTERVAL {
                    if let Err(e) = self.socket.send_keepalive(connection) {
                        *client = ClientState::Disconnecting(ServerDisconnectReason::SocketError(e.kind()));
                        continue;
                    }
                }
                if connection.last_packet_received() > CONNECTION_TIMEOUT {
                    *client = ClientState::Disconnecting(ServerDisconnectReason::TimedOut);
                }
            }
        }
    }

    pub fn next_event<'a>(&mut self, payload: &'a mut [u8]) -> IOResult<Option<ServerEvent<'a>>> {
        if let Some((client, seq)) = self.ack_queue.pop_front() {
            return Ok(Some(ServerEvent::PacketAcknowledged(client, seq)))
        }

        for (id, client) in self.clients.slots_mut() {
            if let ClientState::Disconnecting(reason) = client {
                let reason = reason.clone();
                *client = ClientState::Disconnected;
                return Ok(Some(ServerEvent::ClientDisconnected(id, reason)));
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
                        if let Some(latest) = conn.handle_seq(seq) {
                            let id = conn.id();
                            conn.handle_ack(ack, |i|self.ack_queue.push_back((id, i)));
                            conn.on_receive();
                            let result = &mut payload[..data.len()];
                            result.copy_from_slice(data);
                            Ok(Some(ServerEvent::PacketReceived(id, latest, result)))
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
                        self.clients.set(id, ClientState::Disconnected);
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

    pub fn send(&mut self, client_id: u16, payload: &[u8]) -> Result<SequenceNumber, ConnectionError> {
        let connection = self.clients.get_connection_mut(client_id)?;
        Ok(self.socket.send_payload(payload, connection)?)
    }

    pub fn disconnect(&mut self, client_id: u16) -> Result<(), ConnectionError> {
        let connection = self.clients.get_connection_mut(client_id)?;
        for _ in 0..10 {
            self.socket.send_with(Packet::Disconnect, connection)?
        }
        let id = connection.id();
        self.clients.set(id, ClientState::Disconnecting(ServerDisconnectReason::Disconnected));
        Ok(())
    }

    pub fn next_sequence_number(&self, client_id: u16) -> Result<SequenceNumber, ConnectionError> {
        Ok(self.clients.get_connection(client_id)?.peek_next_sequence_number())
    }

    pub fn connection(&self, client_id: u16) -> Result<&VirtualConnection, ConnectionError> {
        self.clients.get_connection(client_id)
    }

}

