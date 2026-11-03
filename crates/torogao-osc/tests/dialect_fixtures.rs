//! Dialect fixtures: packets in the exact byte shapes
//! VRChat tooling emits, per the published documentation
//! (docs.vrchat.com, "OSC Avatar Parameters" and "OSC as
//! Input Controller"), decoded and routed against a
//! declared surface. The first fixtures are written as
//! literal wire bytes so the wire form itself is pinned,
//! not just the codec's opinion of it.

use torogao_osc::codec::{OscArg, OscMessage, encode};
use torogao_osc::surface::{Direction, InputValue, ParamType, ParamValue, Surface, SurfaceEvent};

/// The surface a small avatar would declare: two custom
/// parameters in, one inout, one reserved-style
/// outbound-only parameter, and the documented inputs.
fn fixture_surface() -> Surface {
    Surface::builder()
        .documented_inputs()
        .parameter("AFK", ParamType::Bool, Direction::InOut)
        .parameter("Face", ParamType::Int, Direction::In)
        .parameter("Smile", ParamType::Float, Direction::InOut)
        .parameter("VelocityX", ParamType::Float, Direction::Out)
        .parameter("VRCEmote", ParamType::Int, Direction::In)
        .build()
        .expect("fixture surface builds")
}

fn route(packet: &[u8]) -> SurfaceEvent {
    let message = torogao_osc::codec::decode(packet).expect("fixture packet decodes");
    fixture_surface()
        .route(&message)
        .expect("fixture message routes")
}

#[test]
fn avatar_change_fixture_is_byte_exact() {
    // "/avatar/change" (14) + NUL + pad = 16 bytes;
    // ",s" + NUL + pad = 4 bytes; the avatar ID (41) +
    // NUL + pad = 44 bytes. Total 64.
    let mut packet = Vec::new();
    packet.extend_from_slice(b"/avatar/change\0\0");
    packet.extend_from_slice(b",s\0\0");
    packet.extend_from_slice(b"avtr_9d58037b-23c7-4c9c-adbd-b1338178cd81");
    packet.extend_from_slice(b"\0\0\0");
    assert_eq!(packet.len(), 64);
    assert_eq!(
        route(&packet),
        SurfaceEvent::AvatarChanged {
            avatar_id: "avtr_9d58037b-23c7-4c9c-adbd-b1338178cd81".to_string(),
        }
    );
}

#[test]
fn int_parameter_fixture_is_byte_exact() {
    // "/avatar/parameters/VRCEmote" (27) + NUL = 28
    // bytes (already on the boundary); ",i" + NUL + pad
    // = 4 bytes; int 5 BE.
    let mut packet = Vec::new();
    packet.extend_from_slice(b"/avatar/parameters/VRCEmote\0");
    packet.extend_from_slice(b",i\0\0");
    packet.extend_from_slice(&5_i32.to_be_bytes());
    assert_eq!(packet.len(), 36);
    assert_eq!(
        route(&packet),
        SurfaceEvent::Parameter {
            name: "VRCEmote".to_string(),
            value: ParamValue::Int(5),
        }
    );
}

#[test]
fn axis_fixture_is_byte_exact() {
    // "/input/Vertical" (15) + NUL = 16 bytes; ",f" +
    // NUL + pad = 4 bytes; float -1.0 BE.
    let mut packet = Vec::new();
    packet.extend_from_slice(b"/input/Vertical\0");
    packet.extend_from_slice(b",f\0\0");
    packet.extend_from_slice(&(-1.0_f32).to_be_bytes());
    assert_eq!(packet.len(), 24);
    assert_eq!(
        route(&packet),
        SurfaceEvent::Input {
            name: "Vertical".to_string(),
            value: InputValue::Axis(-1.0),
        }
    );
}

#[test]
fn bool_parameter_fixture_is_byte_exact() {
    // "/avatar/parameters/AFK" (22) + NUL + pad = 24
    // bytes; ",T" + NUL + pad = 4 bytes; bools carry no
    // payload bytes. Total 28.
    let mut packet = Vec::new();
    packet.extend_from_slice(b"/avatar/parameters/AFK\0\0");
    packet.extend_from_slice(b",T\0\0");
    assert_eq!(packet.len(), 28);
    assert_eq!(
        route(&packet),
        SurfaceEvent::Parameter {
            name: "AFK".to_string(),
            value: ParamValue::Bool(true),
        }
    );
}

#[test]
fn float_parameter_and_button_fixtures_route() {
    let float_packet = encode(&OscMessage {
        address: "/avatar/parameters/Smile".to_string(),
        args: vec![OscArg::Float(0.25)],
    })
    .expect("encode");
    assert_eq!(
        route(&float_packet),
        SurfaceEvent::Parameter {
            name: "Smile".to_string(),
            value: ParamValue::Float(0.25),
        }
    );
    let button_packet = encode(&OscMessage {
        address: "/input/Jump".to_string(),
        args: vec![OscArg::Int(1)],
    })
    .expect("encode");
    assert_eq!(
        route(&button_packet),
        SurfaceEvent::Input {
            name: "Jump".to_string(),
            value: InputValue::Button(true),
        }
    );
}

#[test]
fn chatbox_fixtures_route_with_documented_defaults() {
    // Two-arg form: the notification flag defaults true.
    let packet = encode(&OscMessage {
        address: "/chatbox/input".to_string(),
        args: vec![OscArg::Str("hello".to_string()), OscArg::Bool(true)],
    })
    .expect("encode");
    assert_eq!(
        route(&packet),
        SurfaceEvent::ChatboxInput {
            notify: true,
            send_immediately: true,
            text: "hello".to_string(),
        }
    );
    // Three-arg form: the flag is taken from the wire.
    let packet = encode(&OscMessage {
        address: "/chatbox/input".to_string(),
        args: vec![
            OscArg::Str("quiet".to_string()),
            OscArg::Bool(false),
            OscArg::Bool(false),
        ],
    })
    .expect("encode");
    assert_eq!(
        route(&packet),
        SurfaceEvent::ChatboxInput {
            notify: false,
            send_immediately: false,
            text: "quiet".to_string(),
        }
    );
    let typing = encode(&OscMessage {
        address: "/chatbox/typing".to_string(),
        args: vec![OscArg::Bool(true)],
    })
    .expect("encode");
    assert_eq!(route(&typing), SurfaceEvent::ChatboxTyping { typing: true });
}
