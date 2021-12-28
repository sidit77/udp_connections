use std::error::Error;
use std::fmt::{Display, Formatter};

pub type IOResult<T> = std::io::Result<T>;

#[derive(Debug)]
pub enum ConnectionError {
    Disconnected,
    ConnectionNotReady
}

impl Display for ConnectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionError::Disconnected => f.write_str("Connection could not be found"),
            ConnectionError::ConnectionNotReady => f.write_str("Connection is not ready")
        }
    }
}

impl Error for ConnectionError {}