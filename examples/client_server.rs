use std::time::Duration;
use byteorder::{BigEndian, ReadBytesExt};
use udp_connections::{ClientEvent, MAX_PAYLOAD_SIZE, ServerEvent, UdpClient, UdpServer};

const SERVER: &str = "127.0.0.1:23452";

fn client() {
    std::thread::sleep(Duration::from_secs_f32(0.5));
    let mut socket = UdpClient::new("udp_connections_demo").unwrap();
    let prefix = format!("[Client {}]", socket.local_addr().unwrap());
    println!("{} starting up", prefix);
    socket.connect(SERVER).unwrap();

    let mut buffer = [0u8; MAX_PAYLOAD_SIZE];
    let mut i = 0u32;
    'outer: loop {
        while let Some(event) = socket.next_event(&mut buffer).unwrap() {
            match event {
                ClientEvent::Connected(id) => println!("{} Connected as {}", prefix, id),
                ClientEvent::Disconnected(reason) => {
                    println ! ("{} Disconnected: {:?}", prefix, reason);
                    break 'outer
                },
                ClientEvent::Packet(mut payload) => {
                    let val = payload.read_u32::<BigEndian>().unwrap();
                    println ! ("{} Packet {}", prefix, val);
                }
            }
        }

        if socket.is_connected() {
            if i % 10 == 0 {
                socket.send(&(i / 10).to_be_bytes()).unwrap();
                if i > 60 {
                    socket.disconnect().unwrap();
                }
            }
            i += 1;
        }

        std::thread::sleep(Duration::from_secs_f32(0.05));
    }

    std::thread::sleep(Duration::from_millis(100));

    println!("{} shutting down", prefix);
}

fn main(){
    let _ = std::thread::spawn(self::client);
    //let _ = std::thread::spawn(self::client);

    let mut socket = UdpServer::listen(SERVER, "udp_connections_demo", 1).unwrap();
    let prefix = format!("[Server {}]", socket.local_addr().unwrap());

    //let mut i = 0u32;
    let mut buffer = [0u8; MAX_PAYLOAD_SIZE];
    loop {
        while let Some(event) = socket.next_event(&mut buffer).unwrap() {
            match event {
                ServerEvent::ClientConnected(client_id) =>
                    println!("{} Client {} connected", prefix, client_id),
                ServerEvent::ClientDisconnected(client_id, reason) =>
                    println!("{} Client {} disconnected: {:?}", prefix, client_id, reason),
                ServerEvent::Packet(client_id, mut payload) => {
                    let val = payload.read_u32::<BigEndian>().unwrap();
                    println!("{} Packet {} from {}", prefix, val, client_id);
                    socket.send(client_id, &val.to_be_bytes()).unwrap();
                }
            }
        }
        //if i % 10 == 0 {
        //    socket.broadcast(&i.to_be_bytes()).unwrap();
        //}
        //i += 1;
        std::thread::sleep(Duration::from_secs_f32(0.05));
    }

}