use std::time::Duration;
use udpcon::{ClientEvent, ServerEvent, UdpClient, UdpServer};

const SERVER: &str = "127.0.0.1:23452";

fn client() {
    std::thread::sleep(Duration::from_secs_f32(0.5));
    let mut socket = UdpClient::new().unwrap();
    let prefix = format!("[Client {}]", socket.local_addr().unwrap());
    println!("{} starting up", prefix);
    socket.connect(SERVER).unwrap();

    let mut buffer = [0u8; udpcon::MAX_PAYLOAD_SIZE];
    let mut i = 0u32;
    'outer: loop {
        loop {
            match socket.next_event(&mut buffer).unwrap() {
                None => break,
                Some(event) => match event {
                    ClientEvent::Connected(id) => println!("{} Connected as {}", prefix, id),
                    ClientEvent::Disconnected(reason) => {
                        println ! ("{} Disconnected: {:?}", prefix, reason);
                        break 'outer
                    },
                    ClientEvent::Packet(size) => {
                        let payload = & buffer[..size];
                        println ! ("{} Packet {:?}", prefix, payload);
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

    std::thread::sleep(Duration::from_millis(100));

    println!("{} shutting down", prefix);
}

fn main(){
    let _ = std::thread::spawn(self::client);
    //let _ = std::thread::spawn(self::client);

    let mut socket = UdpServer::listen(SERVER, 1).unwrap();
    let prefix = format!("[Server {}]", socket.local_addr().unwrap());

    //let mut i = 0u32;
    let mut buffer = [0u8; udpcon::MAX_PAYLOAD_SIZE];
    loop {
        loop {
            match socket.next_event(&mut buffer).unwrap() {
                None => break,
                Some(event) => match event {
                    ServerEvent::ClientConnected(client_id) =>
                        println!("{} Client {} connected", prefix, client_id),
                    ServerEvent::ClientDisconnected(client_id, reason) =>
                        println!("{} Client {} disconnected: {:?}", prefix, client_id, reason),
                    ServerEvent::Packet(client_id, size) => {
                        let payload = &buffer[..size];
                        println!("{} Packet {:?} from {}", prefix, payload, client_id);
                        socket.send(client_id, payload).unwrap();
                    }
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