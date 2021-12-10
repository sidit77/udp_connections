use std::collections::VecDeque;
use std::net::{SocketAddr, ToSocketAddrs};
use std::io::{Error, ErrorKind, Result};
use std::time::Instant;
use crate::connection::{PacketSocket, VirtualConnection};
use crate::constants::{CONNECTION_TIMEOUT, KEEPALIVE_INTERVAL};
use crate::packets::Packet;
use crate::sequencing::SequenceNumber;
use crate::socket::UdpSocketImpl;

#[derive(Debug, Copy, Clone)]
pub enum ClientDisconnectReason {
    Disconnected,
    TimedOut,
    ConnectionDenied
}

#[derive(Debug, Copy, Clone)]
pub enum ClientEvent<'a> {
    Connected(u16),
    Disconnected(ClientDisconnectReason),
    PacketReceived(&'a [u8]),
    PacketAcknowledged(SequenceNumber)
}

#[derive(Debug, Clone)]
enum ClientState {
    Disconnected,
    Connecting(SocketAddr, Instant),
    Connected(VirtualConnection),
    Disconnecting
}

#[derive(Debug)]
pub struct Client<U: UdpSocketImpl> {
    socket: PacketSocket<U>,
    state: ClientState,
    ack_queue: VecDeque<SequenceNumber>
}

impl<U: UdpSocketImpl> Client<U> {

    pub fn from_socket(socket: U, identifier: &str) -> Self{
        Self {
            socket: PacketSocket::new(socket, identifier),
            state: ClientState::Disconnected,
            ack_queue: VecDeque::new()
        }
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn remote_addr(&self) -> Option<SocketAddr> {
        match &self.state {
            ClientState::Disconnected => None,
            ClientState::Connecting(addrs, _) => Some(*addrs),
            ClientState::Connected(vc) => Some(vc.addrs()),
            ClientState::Disconnecting => None,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.state, ClientState::Connected{..})
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
        match &mut self.state {
            ClientState::Connected(vc) => {
                for _ in 0..10 {
                    self.socket.send_with (Packet::Disconnect, vc)?;
                }
                self.state = ClientState::Disconnecting;
                Ok(())
            },
            _ => Err(Error::new(ErrorKind::NotConnected, "not connected to a server!"))
        }
    }

    pub fn update(&mut self) -> Result<()> {
        let now = Instant::now();
        match self.state {
            ClientState::Connecting(remote, _)
            => self.socket.send_to(Packet::ConnectionRequest, remote),
            ClientState::Connected(ref mut vc) if (now - vc.last_send_packet) > KEEPALIVE_INTERVAL
            => self.socket.send_keepalive(vc),
            _ => Ok(())
        }
    }

    pub fn next_event<'a>(&mut self, payload: &'a mut [u8]) -> Result<Option<ClientEvent<'a>>> {
        let now = Instant::now();

        if let Some(seq) = self.ack_queue.pop_front() {
            return Ok(Some(ClientEvent::PacketAcknowledged(seq)))
        }

        match &self.state {
            ClientState::Connecting(_, start) => if (now - *start) > CONNECTION_TIMEOUT {
                self.state = ClientState::Disconnected;
                return Ok(Some(ClientEvent::Disconnected(ClientDisconnectReason::TimedOut)))
            },
            ClientState::Connected(vc) if (now - vc.last_received_packet) > CONNECTION_TIMEOUT => {
                self.state = ClientState::Disconnected;
                return Ok(Some(ClientEvent::Disconnected(ClientDisconnectReason::TimedOut)))
            },
            ClientState::Disconnecting => {
                self.state = ClientState::Disconnected;
                return Ok(Some(ClientEvent::Disconnected(ClientDisconnectReason::Disconnected)))
            }
            _ => {}
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
                        let result = &mut payload[..data.len()];
                        result.copy_from_slice(data);
                        vc.on_receive();
                        vc.handle_seq(seq);
                        vc.handle_ack(ack, |i|self.ack_queue.push_back(i));
                        Ok(Some(ClientEvent::PacketReceived(result)))
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

    pub fn send(&mut self, payload: &[u8]) -> Result<SequenceNumber> {
        match &mut self.state {
            ClientState::Connected(vc) => self.socket.send_payload(payload, vc),
            _ => Err(Error::new(ErrorKind::NotConnected, "not connected to a server"))
        }
    }

}
