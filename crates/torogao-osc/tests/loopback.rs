//! Loopback transport tests: real UDP sockets on
//! ephemeral localhost ports. Validation tests prove
//! events and publications cross the wire intact;
//! adversarial tests prove malformed datagrams and
//! floods are counted and survived, never panicked on
//! and never blocking the loop.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use torogao_osc::codec::{OscArg, OscMessage, decode, encode};
use torogao_osc::net::{EVENT_QUEUE_CAPACITY, Listener, OscConfig, Publisher};
use torogao_osc::surface::{
    Direction, InputValue, OscSink, ParamType, ParamValue, Surface, SurfaceEvent,
};

#[derive(Default)]
struct RecordingSink {
    events: Vec<SurfaceEvent>,
}

impl OscSink for RecordingSink {
    fn on_event(&mut self, event: SurfaceEvent) {
        self.events.push(event);
    }
}

fn test_surface() -> Surface {
    Surface::builder()
        .documented_inputs()
        .parameter("Smile", ParamType::Float, Direction::InOut)
        .parameter("Face", ParamType::Int, Direction::In)
        .build()
        .expect("test surface builds")
}

fn fast_config(receive_port: u16, send_port: u16) -> OscConfig {
    OscConfig {
        receive_poll: Duration::from_millis(5),
        ..OscConfig::loopback(receive_port, send_port)
    }
}

fn wait_until(deadline: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    condition()
}

fn parameter_packet(name: &str, value: f32) -> Vec<u8> {
    encode(&OscMessage {
        address: format!("/avatar/parameters/{name}"),
        args: vec![OscArg::Float(value)],
    })
    .expect("encode")
}

#[test]
fn loopback_round_trip_delivers_events_in_order() {
    let listener = Listener::spawn(test_surface(), fast_config(0, 0)).expect("listener spawns");
    let target = listener.local_addr();
    let sender = UdpSocket::bind("127.0.0.1:0").expect("sender binds");
    sender
        .send_to(&parameter_packet("Smile", 0.5), target)
        .expect("send one");
    sender
        .send_to(
            &encode(&OscMessage {
                address: "/avatar/change".to_string(),
                args: vec![OscArg::Str("avtr_loop".to_string())],
            })
            .expect("encode"),
            target,
        )
        .expect("send two");

    let mut sink = RecordingSink::default();
    let drained = wait_until(Duration::from_secs(5), || {
        listener.poll_events(&mut sink, 16);
        sink.events.len() == 2
    });
    assert!(drained, "both events arrived: {:?}", sink.events);
    assert_eq!(
        sink.events[0],
        SurfaceEvent::Parameter {
            name: "Smile".to_string(),
            value: ParamValue::Float(0.5),
        }
    );
    assert_eq!(
        sink.events[1],
        SurfaceEvent::AvatarChanged {
            avatar_id: "avtr_loop".to_string(),
        }
    );
    let stats = listener.stats();
    assert_eq!(stats.datagrams_received, 2);
    assert_eq!(stats.events_delivered, 2);
    listener.shutdown();
}

#[test]
fn malformed_datagrams_are_counted_and_the_loop_survives() {
    let listener = Listener::spawn(test_surface(), fast_config(0, 0)).expect("listener spawns");
    let target = listener.local_addr();
    let sender = UdpSocket::bind("127.0.0.1:0").expect("sender binds");
    // Non-OSC garbage.
    sender.send_to(b"\xde\xad\xbe\xef", target).expect("send");
    // A truncated parameter packet (cut mid-float).
    let packet = parameter_packet("Smile", 0.5);
    sender
        .send_to(&packet[..packet.len() - 2], target)
        .expect("send");
    // A bundle head, which this dialect never carries.
    let mut bundle = Vec::new();
    bundle.extend_from_slice(b"#bundle\0");
    bundle.extend_from_slice(&[0; 8]);
    sender.send_to(&bundle, target).expect("send");
    // Then a valid packet: the loop must still serve it.
    sender
        .send_to(&parameter_packet("Smile", 0.75), target)
        .expect("send");

    let mut sink = RecordingSink::default();
    let served = wait_until(Duration::from_secs(5), || {
        listener.poll_events(&mut sink, 16);
        sink.events.len() == 1 && listener.stats().packets_malformed == 3
    });
    assert!(served, "stats: {:?}", listener.stats());
    assert_eq!(
        sink.events[0],
        SurfaceEvent::Parameter {
            name: "Smile".to_string(),
            value: ParamValue::Float(0.75),
        }
    );
    listener.shutdown();
}

#[test]
fn flood_drops_are_counted_and_bounded_by_the_queue() {
    let listener = Listener::spawn(test_surface(), fast_config(0, 0)).expect("listener spawns");
    let target = listener.local_addr();
    let sender = UdpSocket::bind("127.0.0.1:0").expect("sender binds");
    let packet = parameter_packet("Smile", 0.5);
    // Nobody drains during the flood: the queue fills at
    // EVENT_QUEUE_CAPACITY and later events must drop.
    // Sends are paced in small batches so the kernel's
    // receive buffer is never itself the dropper — the
    // drops under test are the queue's, and every sent
    // datagram must arrive and be accounted.
    let sent: u64 = 512;
    for _batch in 0..16 {
        for _ in 0..32 {
            sender.send_to(&packet, target).expect("send");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let settled = wait_until(Duration::from_secs(30), || {
        listener.stats().datagrams_received == sent
    });
    assert!(settled, "stats: {:?}", listener.stats());
    let stats = listener.stats();
    assert_eq!(stats.events_delivered, EVENT_QUEUE_CAPACITY as u64);
    assert_eq!(
        stats.events_queue_dropped,
        sent - EVENT_QUEUE_CAPACITY as u64,
        "every datagram past capacity drops: {stats:?}"
    );
    assert_eq!(
        stats.events_delivered + stats.events_queue_dropped,
        stats.datagrams_received,
        "every received datagram is accounted: {stats:?}"
    );
    let mut sink = RecordingSink::default();
    let drained = listener.poll_events(&mut sink, 1024);
    assert_eq!(drained as u64, stats.events_delivered);
    listener.shutdown();
}

#[test]
fn publisher_sends_dialect_shapes_to_the_peer() {
    let surface = test_surface();
    let peer = UdpSocket::bind("127.0.0.1:0").expect("peer binds");
    peer.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("peer timeout");
    let peer_addr = peer.local_addr().expect("peer addr");
    let config = OscConfig {
        peer_addr,
        ..fast_config(0, 0)
    };
    let publisher = Publisher::bind(surface, config).expect("publisher binds");

    publisher
        .publish_parameter("Smile", ParamValue::Float(0.5))
        .expect("publish parameter");
    publisher
        .publish_avatar_change("avtr_pub")
        .expect("publish change");
    publisher
        .publish_input("Jump", InputValue::Button(true))
        .expect("publish input");
    assert_eq!(publisher.stats().messages_sent, 3);

    let mut buffer = [0_u8; 2048];
    let (len, _) = peer.recv_from(&mut buffer).expect("peer receives one");
    let message = decode(&buffer[..len]).expect("peer decodes one");
    assert_eq!(message.address, "/avatar/parameters/Smile");
    assert_eq!(message.args, vec![OscArg::Float(0.5)]);
    let (len, _) = peer.recv_from(&mut buffer).expect("peer receives two");
    let message = decode(&buffer[..len]).expect("peer decodes two");
    assert_eq!(message.address, "/avatar/change");
    assert_eq!(message.args, vec![OscArg::Str("avtr_pub".to_string())]);
    let (len, _) = peer.recv_from(&mut buffer).expect("peer receives three");
    let message = decode(&buffer[..len]).expect("peer decodes three");
    assert_eq!(message.address, "/input/Jump");
    // Buttons travel as the dialect's int 1/0.
    assert_eq!(message.args, vec![OscArg::Int(1)]);
}

#[test]
fn publisher_refuses_undeclared_and_wrong_direction_silently_on_the_wire() {
    let surface = test_surface();
    let peer = UdpSocket::bind("127.0.0.1:0").expect("peer binds");
    peer.set_read_timeout(Some(Duration::from_millis(300)))
        .expect("peer timeout");
    let peer_addr = peer.local_addr().expect("peer addr");
    let config = OscConfig {
        peer_addr,
        ..fast_config(0, 0)
    };
    let publisher = Publisher::bind(surface, config).expect("publisher binds");

    assert!(
        publisher
            .publish_parameter("Ghost", ParamValue::Float(0.0))
            .is_err()
    );
    // "Face" is declared In only: publishing it refuses.
    assert!(
        publisher
            .publish_parameter("Face", ParamValue::Int(1))
            .is_err()
    );
    assert!(
        publisher
            .publish_parameter("Smile", ParamValue::Int(1))
            .is_err()
    );
    assert_eq!(publisher.stats().messages_sent, 0);
    // Nothing reached the wire: the receive times out.
    let mut buffer = [0_u8; 64];
    assert!(peer.recv_from(&mut buffer).is_err());
}
