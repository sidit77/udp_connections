use std::io::{Cursor, Error, ErrorKind, Result, Write};
use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher;

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
    Disconnect,
    Payload(&'a [u8])
}

impl<'a> ClientProtocol<'a> {

    pub(crate) fn from(data: &'a [u8]) -> Result<Self> {
        let mut data = data;
        let checksum = data.read_u32::<NetworkEndian>()?;
        let mut hasher = Hasher::new();
        hasher.update(&PROTOCOL_ID.to_be_bytes());
        hasher.update(data);
        assert(checksum == hasher.finalize(), "bad checksum")?;
        match data.read_u8()? {
            0x00 => Ok(ClientProtocol::ConnectionRequest),
            0x01 => Ok(ClientProtocol::Disconnect),
            0x02 => Ok(ClientProtocol::Payload(data)),
            _ => Err(Error::new(ErrorKind::InvalidData, "Invalid packet id"))
        }
    }

    pub(crate) fn write<'b>(&self, data: &'b mut [u8]) -> Result<&'b [u8]> {
        let mut data = Cursor::new(data);
        data.write_all(&PROTOCOL_ID.to_be_bytes())?;
        match self {
            ClientProtocol::ConnectionRequest => {
                data.write_u8(0x00)?
            },
            ClientProtocol::Disconnect => {
                data.write_u8(0x01)?
            },
            ClientProtocol::Payload(payload) => {
                data.write_u8(0x02)?;
                data.write_all(payload)?;
            }
        }
        let len = data.position() as usize;
        let mut hasher = Hasher::new();
        hasher.update(&data.get_ref()[..len]);
        data.set_position(0);
        data.write_u32::<NetworkEndian>(hasher.finalize())?;
        Ok(&data.into_inner()[..len])
    }

}

#[derive(Debug, PartialEq)]
pub(crate) enum ServerProtocol<'a> {
    ConnectionAccepted(u16),
    ConnectionDenied,
    Disconnect,
    Payload(&'a [u8])
}

impl<'a> ServerProtocol<'a> {

    pub(crate) fn from(data: &'a [u8]) -> Result<Self> {
        let mut data = data;
        let checksum = data.read_u32::<NetworkEndian>()?;
        let mut hasher = Hasher::new();
        hasher.update(&PROTOCOL_ID.to_be_bytes());
        hasher.update(data);
        assert(checksum == hasher.finalize(), "bad checksum")?;
        match data.read_u8()? {
            0x00 => Ok(ServerProtocol::ConnectionAccepted(data.read_u16::<NetworkEndian>()?)),
            0x01 => Ok(ServerProtocol::ConnectionDenied),
            0x02 => Ok(ServerProtocol::Disconnect),
            0x03 => Ok(ServerProtocol::Payload(data)),
            _ => Err(Error::new(ErrorKind::InvalidData, "Invalid packet id"))
        }
    }

    pub(crate) fn write<'b>(&self, data: &'b mut [u8]) -> Result<&'b [u8]> {
        let mut data = Cursor::new(data);
        data.write_all(&PROTOCOL_ID.to_be_bytes())?;
        match *self {
            ServerProtocol::ConnectionAccepted(id) => {
                data.write_u8(0x00)?;
                data.write_u16::<NetworkEndian>(id)?
            },
            ServerProtocol::ConnectionDenied => {
                data.write_u8(0x01)?;
            },
            ServerProtocol::Disconnect => {
                data.write_u8(0x02)?;
            },
            ServerProtocol::Payload(payload) => {
                data.write_u8(0x03)?;
                data.write_all(payload)?;
            }
        }
        let len = data.position() as usize;
        let mut hasher = Hasher::new();
        hasher.update(&data.get_ref()[..len]);
        data.set_position(0);
        data.write_u32::<NetworkEndian>(hasher.finalize())?;
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
            ClientProtocol::Disconnect,
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
    #[should_panic]
    fn test_client_protocol_crc() {
        let mut buffer = [0u8; 10];
        let test = ClientProtocol::ConnectionRequest;
        let len = test.write(&mut buffer).unwrap().len();
        let bin= &mut buffer[..len];
        bin[4] += 1;
        ClientProtocol::from(bin).unwrap();
    }

    #[test]
    fn test_server_protocol() {
        let mut buffer = [0u8; 10];

        let test_cases = [
            ServerProtocol::ConnectionAccepted(3),
            ServerProtocol::ConnectionDenied,
            ServerProtocol::Disconnect,
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

    #[test]
    #[should_panic]
    fn test_server_protocol_crc() {
        let mut buffer = [0u8; 10];
        let test = ServerProtocol::ConnectionDenied;
        let len = test.write(&mut buffer).unwrap().len();
        let bin= &mut buffer[..len];
        bin[4] += 1;
        ServerProtocol::from(bin).unwrap();
    }
}
