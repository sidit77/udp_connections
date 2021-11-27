use std::io::{Cursor, Error, ErrorKind, Result, Write};
use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};

const PROTOCOL_ID: u32 = 850394;

fn assert(v: bool, reason: &str) -> Result<()> {
    if v {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::InvalidData, reason))
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum ClientProtocol<'a> {
    ConnectionRequest,
    Payload(&'a [u8])
}

impl<'a> ClientProtocol<'a> {

    pub(crate) fn from(data: &'a [u8]) -> Result<Self> {
        let mut data = data;
        assert(data.read_u32::<NetworkEndian>()? == PROTOCOL_ID, "bad protocol id")?;
        match data.read_u8()? {
            0x00 => Ok(ClientProtocol::ConnectionRequest),
            0x01 => Ok(ClientProtocol::Payload(data)),
            _ => Err(Error::new(ErrorKind::InvalidData, "Invalid packet id"))
        }
    }

    pub(crate) fn write<'b>(&self, data: &'b mut [u8]) -> Result<&'b [u8]> {
        let mut data = Cursor::new(data);
        data.write_u32::<NetworkEndian>(PROTOCOL_ID)?;
        match self {
            ClientProtocol::ConnectionRequest => {
                data.write_u8(0x00)?
            },
            ClientProtocol::Payload(payload) => {
                data.write_u8(0x01)?;
                data.write_all(payload)?;
            }
        }
        let len = data.position() as usize;
        Ok(&data.into_inner()[..len])
    }

}

#[derive(Debug, PartialEq)]
pub(crate) enum ServerProtocol<'a> {
    ConnectionAccepted,
    ConnectionDenied,
    Payload(&'a [u8])
}

impl<'a> ServerProtocol<'a> {

    pub(crate) fn from(data: &'a [u8]) -> Result<Self> {
        let mut data = data;
        assert(data.read_u32::<NetworkEndian>()? == PROTOCOL_ID, "bad protocol id")?;
        match data.read_u8()? {
            0x00 => Ok(ServerProtocol::ConnectionAccepted),
            0x01 => Ok(ServerProtocol::ConnectionDenied),
            0x02 => Ok(ServerProtocol::Payload(data)),
            _ => Err(Error::new(ErrorKind::InvalidData, "Invalid packet id"))
        }
    }

    pub(crate) fn write<'b>(&self, data: &'b mut [u8]) -> Result<&'b [u8]> {
        let mut data = Cursor::new(data);
        data.write_u32::<NetworkEndian>(PROTOCOL_ID)?;
        match self {
            ServerProtocol::ConnectionAccepted => {
                data.write_u8(0x00)?
            },
            ServerProtocol::ConnectionDenied => {
                data.write_u8(0x01)?
            },
            ServerProtocol::Payload(payload) => {
                data.write_u8(0x02)?;
                data.write_all(payload)?;
            }
        }
        let len = data.position() as usize;
        Ok(&data.into_inner()[..len])
    }

}

#[cfg(test)]
mod tests {
    use crate::protocol::{ClientProtocol, ServerProtocol};

    #[test]
    fn test_client_protocol() {
        let mut buffer = [0u8; 10];

        let test_cases = [
            ClientProtocol::ConnectionRequest,
            ClientProtocol::Payload(&[1,2,3])
        ];

        for test in test_cases {
            print!("Testing {:?}: ", test);
            let bin = test.write(&mut buffer).unwrap();
            let rev = ClientProtocol::from(bin).unwrap();
            assert_eq!(test, rev);
            println!("ok")
        }
    }

    #[test]
    fn test_server_protocol() {
        let mut buffer = [0u8; 10];

        let test_cases = [
            ServerProtocol::ConnectionAccepted,
            ServerProtocol::ConnectionDenied,
            ServerProtocol::Payload(&[1,2,3])
        ];

        for test in test_cases {
            print!("Testing {:?}: ", test);
            let bin = test.write(&mut buffer).unwrap();
            let rev = ServerProtocol::from(bin).unwrap();
            assert_eq!(test, rev);
            println!("ok")
        }
    }
}
