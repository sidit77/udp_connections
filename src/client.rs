use std::collections::VecDeque;
use std::net::SocketAddr;
use std::io::ErrorKind;
use std::time::Instant;
use crate::connection::{PacketSocket, VirtualConnection};
use crate::constants::{CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL};
use crate::error::{ConnectionError, IOResult};
use crate::packets::Packet;
use crate::sequencing::{SequenceNumber, SequenceResult};
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
    PacketAcknowledged(SequenceNumber),
    PacketLost(SequenceNumber)
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
    ack_queue: VecDeque<(SequenceNumber, bool)>
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

    pub fn is_disconnected(&self) -> bool {
        matches!(self.state, ClientState::Disconnected)
    }

    pub fn connect(&mut self, addrs: SocketAddr) {
        self.state = ClientState::Connecting(addrs, Instant::now());
    }

    pub fn disconnect(&mut self) -> Result<(), ConnectionError> {
        let connection = self.state.get_connection_mut()?;
        let mut attempts = 10;
        let reason = loop {
            match self.socket.send_with (Packet::Disconnect, connection) {
                Ok(_) => match attempts {
                    0 | 1 => break ClientDisconnectReason::Disconnected,
                    _ => attempts -= 1
                },
                Err(e) => break ClientDisconnectReason::SocketError(e.kind())
            }
        };
        self.state = ClientState::Disconnecting(reason);
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
        if let Some((seq, acked)) = self.ack_queue.pop_front() {
            match acked {
                true => return Ok(Some(ClientEvent::PacketAcknowledged(seq))),
                false => return Ok(Some(ClientEvent::PacketLost(seq)))
            }
        }

        if let ClientState::Disconnecting(reason) = &self.state {
            let reason = reason.clone();
            self.state = ClientState::Disconnected;
            return Ok(Some(ClientEvent::Disconnected(reason)));
        }

        loop {
            match self.socket.recv_from() {
                Ok((packet, src)) => match self.state {
                    ClientState::Connecting(remote, _) if remote == src => match packet{
                        Ok(Packet::ConnectionAccepted(id)) => {
                            self.state = ClientState::Connected(VirtualConnection::new(src, id));
                            return Ok(Some(ClientEvent::Connected(id)))
                        },
                        Ok(Packet::ConnectionDenied) => {
                            self.state = ClientState::Disconnected;
                            return  Ok(Some(ClientEvent::Disconnected(ClientDisconnectReason::ConnectionDenied)))
                        }
                        _ => continue
                    },
                    ClientState::Connected(ref mut vc) if vc.addrs() == src => match packet{
                        Ok(Packet::Payload(seq, ack, data)) => {
                            let seq = vc.handle_seq(seq);
                            if let SequenceResult::Latest | SequenceResult::Fresh = seq {
                                vc.on_receive();
                                vc.handle_ack(ack, |i, j|self.ack_queue.push_back((i, j)));
                                let result = &mut payload[..data.len()];
                                result.copy_from_slice(data);
                                return Ok(Some(ClientEvent::PacketReceived(seq == SequenceResult::Latest, result)))
                            }
                        },
                        Ok(Packet::KeepAlive(ack)) => {
                            vc.on_receive();
                            vc.handle_ack(ack, |i, j|self.ack_queue.push_back((i, j)));
                        },
                        Ok(Packet::Disconnect) => {
                            self.state = ClientState::Disconnected;
                            return Ok(Some(ClientEvent::Disconnected(ClientDisconnectReason::Disconnected)))
                        },
                        _ => continue
                    }
                    _ => continue
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock) => return Ok(None),
                Err(e) => return Err(e)
            }
        }

    }

    pub fn send(&mut self, payload: &[u8]) -> Result<SequenceNumber, ConnectionError> {
        let connection = self.state.get_connection_mut()?;
        match self.socket.send_payload(payload, connection) {
            Ok(seq) => Ok(seq),
            Err(err) => {
                self.state = ClientState::Disconnecting(ClientDisconnectReason::SocketError(err.kind()));
                Err(ConnectionError::Disconnected)
            }
        }
    }

}
