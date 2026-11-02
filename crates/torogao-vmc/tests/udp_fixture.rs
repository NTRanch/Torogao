//! The UDP receiver fixture (Stage 3.3 gate): a VMC frame
//! sent over a real localhost UDP socket is received,
//! assembled, and asserted value-for-value — the output is
//! proven at the receiving end, not just at the send call.
//! The governed downstream direction is proven the same
//! way: a governed state sent onward is received by a
//! second socket and read back into the identical values.

use std::collections::BTreeMap;
use std::time::Duration;

use torogao_vmc::arkit::{GovernedValue, governed_from_blends};
use torogao_vmc::net::{OscReceiver, OscSender};
use torogao_vmc::vmc::{BonePose, RootPose, VmcFrame};

/// Whether this environment permits UDP sends at all.
/// The sandbox forbids them (EPERM even on loopback);
/// there the send-dependent fixtures skip loudly and the
/// gate runs on primo, the estate's pattern for
/// environment-blocked gates. The probe targets a scratch
/// socket, never the fixture's receiver.
fn udp_send_permitted() -> bool {
    let Ok(scratch) = std::net::UdpSocket::bind("127.0.0.1:0") else {
        return false;
    };
    let Ok(address) = scratch.local_addr() else {
        return false;
    };
    let Ok(sender) = std::net::UdpSocket::bind("127.0.0.1:0") else {
        return false;
    };
    match sender.send_to(b"probe", address) {
        Ok(_) => true,
        Err(err) => err.kind() != std::io::ErrorKind::PermissionDenied,
    }
}

fn performance_frame() -> VmcFrame {
    VmcFrame {
        time_seconds: Some(2.5),
        blends: vec![
            ("eyeBlinkLeft".to_string(), 0.125),
            ("eyeBlinkRight".to_string(), 0.25),
            ("eyeLookInLeft".to_string(), 0.5),
            ("eyeLookUpRight".to_string(), 0.75),
            ("jawOpen".to_string(), 0.375),
            ("mouthSmileLeft".to_string(), 0.625),
            ("mouthSmileRight".to_string(), 0.875),
        ],
        bones: vec![
            BonePose {
                name: "Head".to_string(),
                position: [0.0, 1.62, 0.01],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            BonePose {
                name: "Chest".to_string(),
                position: [0.0, 1.1, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
        ],
        root: Some(RootPose {
            position: [0.25, 0.0, -0.5],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }),
    }
}

#[test]
fn udp_round_trip_delivers_the_whole_frame() {
    if !udp_send_permitted() {
        eprintln!("SKIP: this environment forbids UDP sends; the fixture's gate runs on primo");
        return;
    }
    let mut receiver = OscReceiver::bind("127.0.0.1:0").expect("receiver binds");
    receiver
        .set_timeout(Some(Duration::from_secs(10)))
        .expect("timeout sets");
    let address = receiver.local_addr().expect("local addr");
    let sent = performance_frame();
    let expected = sent.clone();
    let handle = std::thread::spawn(move || receiver.recv_frame());
    let sender = OscSender::to(address).expect("sender binds");
    sender.send_frame(&sent).expect("frame sends");
    let received = handle
        .join()
        .expect("receiver thread completes")
        .expect("receive succeeds")
        .expect("a frame completes");
    assert_eq!(received, expected, "the received frame is the sent frame");
}

#[test]
fn governed_state_reaches_a_downstream_consumer() {
    if !udp_send_permitted() {
        eprintln!("SKIP: this environment forbids UDP sends; the fixture's gate runs on primo");
        return;
    }
    let mut consumer = OscReceiver::bind("127.0.0.1:0").expect("consumer binds");
    consumer
        .set_timeout(Some(Duration::from_secs(10)))
        .expect("timeout sets");
    let address = consumer.local_addr().expect("local addr");
    let mut values: BTreeMap<String, GovernedValue> = BTreeMap::new();
    values.insert("Eye:: Left:: Blink".to_string(), GovernedValue::x(0.5));
    values.insert(
        "Head:: Yaw-Pitch".to_string(),
        GovernedValue::xy(0.25, -0.5),
    );
    values.insert("Mouth:: Shape".to_string(), GovernedValue::xy(0.0, 0.75));
    let expected = values.clone();
    let handle = std::thread::spawn(move || consumer.recv_frame());
    let sender = OscSender::to(address).expect("sender binds");
    sender.send_governed(&values).expect("governed state sends");
    let received = handle
        .join()
        .expect("consumer thread completes")
        .expect("receive succeeds")
        .expect("a frame completes");
    assert_eq!(
        governed_from_blends(&received.blends),
        expected,
        "the consumer reads back the identical governed values"
    );
}

#[test]
fn a_quiet_feed_is_a_timeout_not_an_error() {
    let mut receiver = OscReceiver::bind("127.0.0.1:0").expect("receiver binds");
    receiver
        .set_timeout(Some(Duration::from_millis(150)))
        .expect("timeout sets");
    let received = receiver.recv_frame().expect("timeout is not an error");
    assert!(received.is_none(), "no frame completes on a quiet feed");
}
