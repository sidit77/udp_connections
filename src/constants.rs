use std::time::Duration;
pub const MAX_PACKET_SIZE: usize = 1500;

pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs_f32(0.5);

pub const PACKET_LOST_CUTOFF: u16 = 40;

pub const PL_SMOOTHING_FACTOR: f32 = 0.025;
pub const RTT_SMOOTHING_FACTOR: f32 = 0.1;