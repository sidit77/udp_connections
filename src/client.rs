use std::collections::VecDeque;
use std::net::SocketAddr;
use std::io::ErrorKind;
use std::time::Instant;
use crate::connection::{PacketSocket, VirtualConnection};
use crate::constants::{CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL};
use crate::error::{ConnectionError, IOResult};
use crate::packets::Packet;
use crate::sequencing::SequenceNumber;
use crate::socket::Transport;

#[derive(Debug, Clone)]
pub enum ClientDisconnectReason {
    Disconnected,
    TimedOut,
    ConnectionDenied,
    SocketError(ErrorKind)
}

#[derive(Debug, Clone)]
pub enum ClientEvent<'a> {
    Connected(u16),
    Disconnected(ClientDisconnectReason),
    PacketReceived(bool, &'a [u8]),
    PacketAcknowledged(SequenceNumber)
}

#[derive(Debug, Clone)]
enum ClientState {
    Disconnected,
    Connecting(SocketAddr, Instant),
    Connected(VirtualConnection),
    Disconnecting(ClientDisconnectReason)
}

impl ClientState {
    pub fn get_connection(&self) -> Result<&VirtualConnection, ConnectionError> {
        match &self {
            ClientState::Connected(connection) => Ok(connection),
            ClientState::Disconnected => Err(ConnectionError::Disconnected),
            _ => Err(ConnectionError::ConnectionNotReady)
        }
    }

    fn get_connection_mut(&mut self) -> Result<&mut VirtualConnection, ConnectionError> {
        match self {
            ClientState::Connected(connection) => Ok(connection),
            ClientState::Disconnected => Err(ConnectionError::Disconnected),
            _ => Err(ConnectionError::ConnectionNotReady)
        }
    }
}

#[derive(Debug)]
pub struct Client {
    socket: PacketSocket,
    state: ClientState,
    ack_queue: VecDeque<SequenceNumber>
}

impl Client {

    pub fn new<T: Transport + 'static>(socket: T, identifier: &str) -> Self{
        Self {
            socket: PacketSocket::new(socket, identifier),
            state: ClientState::Disconnected,
            ack_queue: VecDeque::new()
        }
    }

    pub fn local_addr(&self) -> IOResult<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn remote_addr(&self) -> Option<SocketAddr> {
        match &self.state {
            ClientState::Disconnected => None,
            ClientState::Connecting(addrs, _) => Some(*addrs),
            ClientState::Connected(vc) => Some(vc.addrs()),
            ClientState::Disconnecting(_) => None,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.state, ClientState::Connected{..})
    }

    pub fn connect(&mut self, addrs: SocketAddr) {
        self.state = ClientState::Connecting(addrs, Instant::now());
    }

    pub fn disconnect(&mut self) -> Result<(), ConnectionError> {
        let connection = self.state.get_connection_mut()?;
        for _ in 0..10 {
            self.socket.send_with (Packet::Disconnect, connection)?;
        }
        self.state = ClientState::Disconnecting(ClientDisconnectReason::Disconnected);
        Ok(())
    }

    pub fn connection(&self) -> Result<&VirtualConnection, ConnectionError> {
        self.state.get_connection()
    }

    pub fn update(&mut self) {
        match self.state {
            ClientState::Connecting(remote, start) => {
                if start.elapsed() > CONNECTION_TIMEOUT {
                    self.state = ClientState::Disconnecting(ClientDisconnectReason::TimedOut)
                }
                if let Err(e) = self.socket.send_to(Packet::ConnectionRequest, remote) {
                    self.state = ClientState::Disconnecting(ClientDisconnectReason::SocketError(e.kind()));
                }
            }
            ClientState::Connected(ref mut connection) => {
                if connection.last_packet_send() > KEEPALIVE_INTERVAL {
                    if let Err(e) = self.socket.send_keepalive(connection) {
                        self.state = ClientState::Disconnecting(ClientDisconnectReason::SocketError(e.kind()));
                        return;
                    }
                }
                if connection.last_packet_received() > CONNECTION_TIMEOUT {
                    self.state = ClientState::Disconnecting(ClientDisconnectReason::TimedOut);
                }
            }
            _ => {}
        }
    }

    pub fn next_event<'a>(&mut self, payload: &'a mut [u8]) -> IOResult<Option<ClientEvent<'a>>> {
        if let Some(seq) = self.ack_queue.pop_front() {
            return Ok(Some(ClientEvent::PacketAcknowledged(seq)))
        }

        if let ClientState::Disconnecting(reason) = &self.state {
            let reason = reason.clone();
            self.state = ClientState::Disconnected;
            return Ok(Some(ClientEvent::Disconnected(reason)));
        }

        match self.socket.recv_from() {
            Ok((packet, src)) => match self.state {
                ClientState::Connecting(remote, _) if remote == src => match packet{
                    Ok(Packet::ConnectionAccepted(id)) => {
                        self.state = ClientState::Connected(VirtualConnection::new(src, id));
                        Ok(Some(ClientEvent::Connected(id)))
                    },
                    Ok(Packet::ConnectionDenied) => {
                        self.state = ClientState::Disconnected;
                        Ok(Some(ClientEvent::Disconnected(ClientDisconnectReason::ConnectionDenied)))
                    }
                    _ => self.next_event(payload)
                },
                ClientState::Connected(ref mut vc) if vc.addrs() == src => match packet{
                    Ok(Packet::Payload(seq, ack, data)) => {
                        if let Some(latest) = vc.handle_seq(seq) {
                            vc.on_receive();
                            vc.handle_ack(ack, |i|self.ack_queue.push_back(i));
                            let result = &mut payload[..data.len()];
                            result.copy_from_slice(data);
                            Ok(Some(ClientEvent::PacketReceived(latest, result)))
                        } else {
                            self.next_event(payload)
                        }
                    },
                    Ok(Packet::KeepAlive(ack)) => {
                        vc.on_receive();
                        vc.handle_ack(ack, |i|self.ack_queue.push_back(i));
                        self.next_event(payload)
                    },
                    Ok(Packet::Disconnect) => {
                        self.state = ClientState::Disconnected;
                        Ok(Some(ClientEvent::Disconnected(ClientDisconnectReason::Disconnected)))
                    },
                    _ => self.next_event(payload)
                }
                _ => self.next_event(payload)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock) => Ok(None),
            Err(e) => Err(e)
        }
    }

    pub fn send(&mut self, payload: &[u8]) -> Result<SequenceNumber, ConnectionError> {
        let connection = self.state.get_connection_mut()?;
        Ok(self.socket.send_payload(payload, connection)?)
    }

    pub fn next_sequence_number(&self) -> Result<SequenceNumber, ConnectionError> {
        Ok(self.connection()?.peek_next_sequence_number())
    }

}
