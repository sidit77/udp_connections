use std::time::Duration;
use udpcon::{ClientEvent, ServerEvent, UdpClient, UdpServer};

const SERVER: &str = "127.0.0.1:23452";

fn client() {
    std::thread::sleep(Duration::from_secs_f32(0.5));
    let mut socket = UdpClient::new().unwrap();
    println!("Client running on {}", socket.local_addr().unwrap());
    socket.connect(SERVER).unwrap();

    let mut buffer = [0u8; udpcon::MAX_PAYLOAD_SIZE];
    let mut i = 0u32;
    'outer: loop {
        loop {
            match socket.next_event(&mut buffer).unwrap() {
                None => break,
                Some(event) => match event {
                    ClientEvent::Connected(id) => println!("Client connected as {}", id),
                    ClientEvent::Disconnected(reason) => {
                        println ! ("Client disconnected: {:?}", reason);
                        break 'outer
                    },
                    ClientEvent::Packet(size) => {
                        let payload = & buffer[..size];
                        println ! ("Got packet: {:?}", payload);
                    }
                }
            }
        }

        if socket.is_connected() {
            if i % 10 == 0 {
                socket.send(&i.to_be_bytes()).unwrap();
            }
            i += 1;
        }

        std::thread::sleep(Duration::from_secs_f32(0.05));
    }

    println!("Client on {} shutting down", socket.local_addr().unwrap());
}

fn main(){
    let _ = std::thread::spawn(self::client);
    let mut socket = UdpServer::listen(SERVER, 5).unwrap();

    let mut buffer = [0u8; udpcon::MAX_PAYLOAD_SIZE];
    loop {
        loop {
            match socket.next_event(&mut buffer).unwrap() {
                None => break,
                Some(event) => match event {
                    ServerEvent::ClientConnected(client_id) =>
                        println!("Client {} connected", client_id),
                    ServerEvent::ClientDisconnected(client_id, reason) =>
                        println!("Client {} disconnected: {:?}", client_id, reason),
                    ServerEvent::Packet(client_id, size) => {
                        let payload = &buffer[..size];
                        println!("Got packet: {:?} from {}", payload, client_id);
                        socket.send(client_id, payload).unwrap();
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs_f32(0.05));
    }

}