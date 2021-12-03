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
pub(crate) enum Packet<'a> {
    ConnectionRequest,
    ConnectionAccepted(u16),
    ConnectionDenied,
    Disconnect,
    Payload(&'a [u8])
}

impl<'a> Packet<'a> {

    pub(crate) fn from(data: &'a [u8]) -> Result<Self> {
        let mut data = data;
        let checksum = data.read_u32::<NetworkEndian>()?;
        let mut hasher = Hasher::new();
        hasher.update(&PROTOCOL_ID.to_be_bytes());
        hasher.update(data);
        assert(checksum == hasher.finalize(), "bad checksum")?;

        match data.read_u8()? {
            0x00 => Ok(Packet::ConnectionRequest),
            0x01 => Ok(Packet::ConnectionAccepted(data.read_u16::<NetworkEndian>()?)),
            0x02 => Ok(Packet::ConnectionDenied),
            0x03 => Ok(Packet::Disconnect),
            0x04 => Ok(Packet::Payload({
                let len = data.read_u16::<NetworkEndian>()? as usize;
                assert(len == data.len(), "wrong packet size")?;
                data
            })),
            _ => Err(Error::new(ErrorKind::InvalidData, "Invalid packet id"))
        }
    }

    pub(crate) fn write<'b>(&self, data: &'b mut [u8]) -> Result<&'b [u8]> {
        let mut data = Cursor::new(data);
        data.write_all(&PROTOCOL_ID.to_be_bytes())?;

        match self {
            Packet::ConnectionRequest => {
                data.write_u8(0x00)?;
            },
            Packet::ConnectionAccepted(id) => {
                data.write_u8(0x01)?;
                data.write_u16::<NetworkEndian>(*id)?;
            },
            Packet::ConnectionDenied => {
                data.write_u8(0x02)?;
            },
            Packet::Disconnect => {
                data.write_u8(0x03)?;
            },
            Packet::Payload(payload) => {
                data.write_u8(0x04)?;
                data.write_u16::<NetworkEndian>(payload.len() as u16)?;
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
    use crate::protocol::{Packet};

    #[test]
    fn test_packets() {
        let mut buffer = [0u8; 10];

        let test_cases = [
            Packet::ConnectionRequest,
            Packet::ConnectionAccepted(45),
            Packet::ConnectionDenied,
            Packet::Disconnect,
            Packet::Payload(&[1,2,3])
        ];

        for test in test_cases {
            print!("Testing {:?}: ", test);
            let bin = test.write(&mut buffer).unwrap();
            let rev = Packet::from(bin).unwrap();
            assert_eq!(test, rev);
            println!("ok")
        }
    }

    #[test]
    #[should_panic]
    fn test_packet_crc() {
        let mut buffer = [0u8; 10];
        let test = Packet::ConnectionRequest;
        let len = test.write(&mut buffer).unwrap().len();
        let bin= &mut buffer[..len];
        bin[4] += 1;
        Packet::from(bin).unwrap();
    }

}
