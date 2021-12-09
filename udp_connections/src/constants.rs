use std::time::Duration;

pub const MAX_PACKET_SIZE: usize = 1500;

pub(crate) const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(1);