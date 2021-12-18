use std::error::Error;
use std::fmt::{Display, Formatter};

pub type IOResult<T> = std::io::Result<T>;
pub type IOError = std::io::Error;

#[derive(Debug)]
pub enum ConnectionError {
    Disconnected,
    ConnectionNotReady,
    SocketError(IOError)
}

impl Display for ConnectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionError::Disconnected => f.write_str("Connection could not be found"),
            ConnectionError::ConnectionNotReady => f.write_str("Connection is not ready"),
            ConnectionError::SocketError(err) => f.write_fmt(format_args!("Socket Error: {}", err))
        }
    }
}

impl Error for ConnectionError {}

impl From<IOError> for ConnectionError {
    fn from(err: IOError) -> Self {
        ConnectionError::SocketError(err)
    }
}
