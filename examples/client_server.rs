use std::collections::HashMap;
use std::net::UdpSocket;
use std::time::{Duration, Instant};
use byteorder::{BigEndian, ReadBytesExt};
use udp_connections::{Client, ClientEvent, Endpoint, MAX_PACKET_SIZE, MessageChannel, NetworkOptions, Server, ServerEvent, TransportExtension};

const SERVER: &str = "127.0.0.1:23452";
const IDENTIFIER: &str = "udp_connections_demo";
const NETWORK_CONFIG: NetworkOptions = NetworkOptions {
    packet_loss: 0.25
};

fn client() {
    std::thread::sleep(Duration::from_secs_f32(0.5));
    let socket = UdpSocket::bind(Endpoint::local_any()).unwrap();
    socket.set_nonblocking(true).unwrap();
    let mut socket = Client::new(socket.with_options(NETWORK_CONFIG), IDENTIFIER);
    let prefix = format!("[Client {}]", socket.local_addr().unwrap());
    println!("{} starting up", prefix);
    socket.connect(SERVER.parse().unwrap());

    let mut msg_channel = None;
    let mut buffer = [0u8; MAX_PACKET_SIZE];
    let mut i = 1u32;
    let mut last_message = Instant::now();
    'outer: loop {
        socket.update();
        while let Some(event) = socket.next_event(&mut buffer).unwrap() {
            match event {
                ClientEvent::Connected(id) => {
                    println!("{} Connected as {}", prefix, id);
                    msg_channel = Some(MessageChannel::new());
                },
                ClientEvent::Disconnected(reason) => {
                    println ! ("{} Disconnected: {:?}", prefix, reason);
                    msg_channel = None;
                    break 'outer
                },
                ClientEvent::PacketReceived(_, payload) => {
                    //let val = payload.read_u32::<BigEndian>().unwrap();
                    let mc = &mut msg_channel.as_mut().unwrap();
                    mc.on_receive(payload).unwrap();
                    while let Some(packet) = mc.receive_message() {
                        let mut packet= packet.as_ref();
                        let val = packet.read_u32::<BigEndian>().unwrap();
                        let connection = socket.connection().unwrap();
                        println ! ("{} Packet {} ({} ms / {:.2} pl)", prefix, val, connection.rtt(), connection.packet_loss());
                        //if val >= 100 {
                        //    socket.disconnect().unwrap();
                        //}
                    }

                },
                ClientEvent::PacketAcknowledged(seq) => {
                    //println!("{} got acknowledged", seq);
                    msg_channel.as_mut().unwrap().on_ack(seq);
                }
                ClientEvent::PacketLost(_) => {}
            }
        }

        if let Some(mc) = msg_channel.as_mut(){
            if mc.has_unsend_messages() {
                let seq = socket.connection().unwrap().peek_next_sequence_number();
                socket.send(mc.send_packets(seq).unwrap()).unwrap();
            }
        }


        if socket.is_connected() {
            if last_message.elapsed() >= Duration::from_secs_f32(0.5) {
                msg_channel.as_mut().unwrap().queue_message(&i.to_be_bytes()).unwrap();
                last_message = Instant::now();
                i += 1;
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(msg_channel.is_none());
    std::thread::sleep(Duration::from_secs_f32(0.5));

    println!("{} shutting down", prefix);
}

fn main(){
    let c1 = std::thread::spawn(self::client);
    //let _ = std::thread::spawn(self::client);

    let socket = UdpSocket::bind(SERVER).unwrap();
    socket.set_nonblocking(true).unwrap();
    let mut socket = Server::new(socket.with_options(NETWORK_CONFIG), IDENTIFIER, 1);
    let prefix = format!("[Server {}]", socket.local_addr().unwrap());

    let mut message_channels = HashMap::new();
    //let mut i = 0u32;
    let mut buffer = [0u8; MAX_PACKET_SIZE];
    'outer: loop  {
        socket.update();
        while let Some(event) = socket.next_event(&mut buffer).unwrap() {
            match event {
                ServerEvent::ClientConnected(client_id) => {
                    println!("{} Client {} connected", prefix, client_id);
                    message_channels.insert(client_id, MessageChannel::new());
                },
                ServerEvent::ClientDisconnected(client_id, reason) => {
                    println!("{} Client {} disconnected: {:?}", prefix, client_id, reason);
                    message_channels.remove(&client_id);
                    if socket.connected_clients().count() == 0 {
                        break 'outer;
                    }
                },
                ServerEvent::PacketReceived(client_id, _, payload) => {
                    //let val = payload.read_u32::<BigEndian>().unwrap();
                    //println!("{} Packet {} from {}", prefix, val, client_id);
                    //socket.send(client_id, &val.to_be_bytes()).unwrap();
                    let mc = &mut message_channels.get_mut(&client_id).unwrap();
                    mc.on_receive(payload).unwrap();
                    while let Some(packet) = mc.receive_message() {
                        let mut packet= packet.as_ref();
                        let val = packet.read_u32::<BigEndian>().unwrap();
                        // println ! ("{} Packet {} from {}", prefix, val, client_id);
                        mc.queue_message(&val.to_be_bytes()).unwrap();
                    }
                },
                ServerEvent::PacketAcknowledged(client_id, seq) => {
                    message_channels.get_mut(&client_id).unwrap().on_ack(seq);
                }
                ServerEvent::PacketLost(_, _) => {}
            }
        }

        for (id, channel) in message_channels.iter_mut() {
            if channel.has_unsend_messages() {
                let seq = socket.connection(*id).unwrap().peek_next_sequence_number();
                socket.send(*id, channel.send_packets(seq).unwrap()).unwrap();
            }
        }

        //if i % 10 == 0 {
        //    socket.broadcast(&i.to_be_bytes()).unwrap();
        //}
        //i += 1;
        std::thread::sleep(Duration::from_millis(10));
    }

    c1.join().unwrap();
}