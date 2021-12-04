use std::io::{Cursor, Error, ErrorKind, Result, Write};
use byteorder::{NetworkEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher;

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
    KeepAlive,
    Disconnect,
    Payload(&'a [u8])
}

impl<'a> Packet<'a> {

    pub(crate) fn from(data: &'a [u8], salt: &[u8]) -> Result<Self> {
        let mut data = data;
        let checksum = data.read_u32::<NetworkEndian>()?;
        let mut hasher = Hasher::new();
        hasher.update(salt);
        hasher.update(data);
        assert(checksum == hasher.finalize(), "bad checksum")?;

        match data.read_u8()? {
            0x00 => Ok(Packet::ConnectionRequest),
            0x01 => Ok(Packet::ConnectionAccepted(data.read_u16::<NetworkEndian>()?)),
            0x02 => Ok(Packet::ConnectionDenied),
            0x03 => Ok(Packet::KeepAlive),
            0x04 => Ok(Packet::Disconnect),
            0x05 => Ok(Packet::Payload({
                let len = data.read_u16::<NetworkEndian>()? as usize;
                assert(len == data.len(), "wrong packet size")?;
                data
            })),
            _ => Err(Error::new(ErrorKind::InvalidData, "Invalid packet id"))
        }
    }

    pub(crate) fn write<'b>(&self, data: &'b mut [u8], salt: &[u8]) -> Result<&'b [u8]> {
        let mut data = Cursor::new(data);
        data.write_u32::<NetworkEndian>(0)?;
        let len1 = data.position() as usize;

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
            Packet::KeepAlive => {
                data.write_u8(0x03)?;
            },
            Packet::Disconnect => {
                data.write_u8(0x04)?;
            },
            Packet::Payload(payload) => {
                data.write_u8(0x05)?;
                data.write_u16::<NetworkEndian>(payload.len() as u16)?;
                data.write_all(payload)?;
            }
        }
        let len2 = data.position() as usize;
        let mut hasher = Hasher::new();
        hasher.update(&salt);
        hasher.update(&data.get_ref()[len1..len2]);
        data.set_position(0);
        data.write_u32::<NetworkEndian>(hasher.finalize())?;
        Ok(&data.into_inner()[..len2])
    }

}

#[cfg(test)]
mod tests {
    use crate::protocol::{Packet};

    const SALT: [u8; 4] = 123456u32.to_be_bytes();

    #[test]
    fn test_packets() {
        let mut buffer = [0u8; 10];

        let test_cases = [
            Packet::ConnectionRequest,
            Packet::ConnectionAccepted(45),
            Packet::ConnectionDenied,
            Packet::KeepAlive,
            Packet::Disconnect,
            Packet::Payload(&[1,2,3])
        ];

        for test in test_cases {
            print!("Testing {:?}: ", test);
            let bin = test.write(&mut buffer, &SALT).unwrap();
            let rev = Packet::from(bin, &SALT).unwrap();
            assert_eq!(test, rev);
            println!("ok")
        }
    }

    #[test]
    #[should_panic]
    fn test_packet_crc() {
        let mut buffer = [0u8; 10];
        let test = Packet::ConnectionRequest;
        let len = test.write(&mut buffer,&SALT).unwrap().len();
        let bin= &mut buffer[..len];
        bin[4] += 1;
        Packet::from(bin,&SALT).unwrap();
    }

}
