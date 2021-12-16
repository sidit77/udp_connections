use std::net::SocketAddr;
use std::io::Result;
use crate::socket::Transport;

#[derive(Debug, Copy, Clone)]
pub struct NetworkOptions {
    pub packet_loss: f32
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            packet_loss: 0.0
        }
    }
}

#[derive(Debug)]
pub struct ConditionedTransport<T: Transport> {
    socket: T,
    options: NetworkOptions,
}

pub trait TransportExtension<T: Transport>: Sized {
    fn with_options(self, options: NetworkOptions) -> ConditionedTransport<T>;
}

impl<T: Transport> TransportExtension<T> for T {
    fn with_options(self, options: NetworkOptions) -> ConditionedTransport<T> {
        ConditionedTransport {
            socket: self,
            options
        }
    }
}

impl<T: Transport> Transport for ConditionedTransport<T> {
    fn send_to(&self, buf: &[u8], addr: SocketAddr) -> Result<usize> {
        self.socket.send_to(buf, addr)
    }

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        match self.socket.recv_from(buf) {
            Ok(result) => if fastrand::f32() < self.options.packet_loss {
                self.recv_from(buf)
            } else {
                Ok(result)
            }
            Err(e) => Err(e)
        }
    }

    fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }
}