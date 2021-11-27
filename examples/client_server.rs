use std::time::Duration;
use udpcon::{ClientEvent, ServerEvent, UdpClient, UdpServer};

const SERVER: &str = "127.0.0.1:23452";

fn client() {
    std::thread::sleep(Duration::from_secs_f32(0.5));
    let mut socket = UdpClient::new().unwrap();
    println!("Client running on {}", socket.local_addr().unwrap());
    socket.connect(SERVER).unwrap();

    let mut buffer = [0u8; udpcon::MAX_PAYLOAD_SIZE];
    loop {
        if let Some(event) = socket.next_event(&mut buffer).unwrap() {
            match event {
                ClientEvent::Connected => println!("Client connected"),
                ClientEvent::Disconnected(reason) => println!("Client disconnected: {:?}", reason),
                ClientEvent::Packet(size) => {
                    let payload = &buffer[..size];
                    println!("Get packet: {:?}", payload);
                }
            }
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
        if let Some(event) = socket.next_event(&mut buffer).unwrap() {
            match event {
                ServerEvent::ClientConnected(client_id) => println!("Client {} connected", client_id),
                ServerEvent::ClientDisconnected(client_id, reason) => println!("Client {} disconnected: {:?}", client_id, reason),
                ServerEvent::Packet(client_id, size) => {
                    let payload = &buffer[..size];
                    println!("Get packet: {:?} from {}", payload, client_id);
                }
            }
        }
        std::thread::sleep(Duration::from_secs_f32(0.05));
    }

}