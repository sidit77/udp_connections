mod packets;
mod socket;
mod client;
mod constants;
mod server;
mod connection;
mod sequencing;
mod reliable;

pub use client::{ClientEvent, ClientDisconnectReason, ClientState};
pub use server::{ServerEvent, ServerDisconnectReason};
pub type ServerState = server::ClientState;
pub use constants::MAX_PACKET_SIZE;
pub use socket::{UdpServer, UdpClient};
pub use reliable::MessageChannel;


#[cfg(feature = "network_simulator")]
mod conditioner;


#[cfg(feature = "network_simulator")]
pub use conditioner::{NetworkOptions, ConditionedUdpClient, ConditionedUdpServer};






