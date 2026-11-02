//! A bounded OSC 1.0 message codec: the wire layer under
//! VMC. One message per datagram (the VMC transport
//! shape): an address pattern, a type-tag string, and the
//! arguments in order. Only the argument kinds VMC traffic
//! carries are modelled — `i` (i32), `f` (f32), `s`
//! (string), `T`/`F` (booleans, no payload bytes).
//!
//! Every size is bounded before allocation: packet bytes,
//! argument count, address and string lengths. Decoding is
//! strict about structure (truncation, bad padding shape,
//! unknown type tags, and trailing bytes all refuse) and
//! lenient only about padding byte values, which the spec
//! leaves to the sender.

use std::fmt;

/// Largest OSC packet, in bytes (the largest UDP payload).
pub const OSC_PACKET_BYTES_MAX: usize = 65_507;
/// Largest argument count in one message.
pub const OSC_ARGS_MAX: usize = 64;
/// Largest address pattern, in bytes (without its NUL).
pub const OSC_ADDRESS_BYTES_MAX: usize = 256;
/// Largest string argument, in bytes (without its NUL).
pub const OSC_STRING_BYTES_MAX: usize = 256;

/// One OSC argument.
#[derive(Debug, Clone, PartialEq)]
pub enum OscArg {
    /// Type tag `T` / `F` (no payload bytes on the wire).
    Bool(bool),
    /// Type tag `f` (big-endian IEEE 754).
    Float(f32),
    /// Type tag `i` (big-endian two's complement).
    Int(i32),
    /// Type tag `s` (NUL-terminated, 4-byte padded).
    Str(String),
}

/// One OSC message: an address pattern plus its arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct OscMessage {
    /// The address pattern (begins with `/`).
    pub address: String,
    /// The arguments, in wire order.
    pub args: Vec<OscArg>,
}

/// Why an OSC packet or message refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscError {
    /// The address does not begin with `/` or carries a NUL.
    AddressInvalid,
    /// The address exceeds `OSC_ADDRESS_BYTES_MAX`.
    AddressTooLong,
    /// More than `OSC_ARGS_MAX` arguments.
    ArgCountOverflow,
    /// A string field's bytes are not valid UTF-8.
    BadUtf8,
    /// The packet does not end on a 4-byte boundary or has
    /// bytes left after the last argument.
    PacketShape,
    /// The packet exceeds `OSC_PACKET_BYTES_MAX`.
    PacketTooLarge,
    /// The packet ends inside a field.
    PacketTruncated,
    /// A string argument exceeds `OSC_STRING_BYTES_MAX`.
    StringTooLong,
    /// A type tag this codec does not model.
    TypeTagUnknown(char),
    /// The type-tag string does not begin with `,`.
    TypeTagShape,
}

impl fmt::Display for OscError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OscError::AddressInvalid => write!(f, "OSC address is invalid"),
            OscError::AddressTooLong => {
                write!(f, "OSC address exceeds {OSC_ADDRESS_BYTES_MAX} bytes")
            }
            OscError::ArgCountOverflow => {
                write!(f, "OSC message exceeds {OSC_ARGS_MAX} arguments")
            }
            OscError::BadUtf8 => write!(f, "OSC string field is not valid UTF-8"),
            OscError::PacketShape => write!(f, "OSC packet has trailing or misaligned bytes"),
            OscError::PacketTooLarge => {
                write!(f, "OSC packet exceeds {OSC_PACKET_BYTES_MAX} bytes")
            }
            OscError::PacketTruncated => write!(f, "OSC packet ends inside a field"),
            OscError::StringTooLong => {
                write!(f, "OSC string exceeds {OSC_STRING_BYTES_MAX} bytes")
            }
            OscError::TypeTagUnknown(tag) => write!(f, "OSC type tag {tag:?} is not modelled"),
            OscError::TypeTagShape => write!(f, "OSC type-tag string does not begin with ','"),
        }
    }
}

impl std::error::Error for OscError {}

/// Pads a field length up to the OSC 4-byte boundary.
fn padded(len: usize) -> usize {
    (len + 3) & !3
}

/// Appends one OSC string field (content + NUL, padded).
fn write_string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
    out.push(0);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

/// Encodes one message. Refuses the same bounds decoding
/// enforces, so a message this crate encodes always
/// decodes.
pub fn encode(message: &OscMessage) -> Result<Vec<u8>, OscError> {
    if !message.address.starts_with('/') || message.address.contains('\0') {
        return Err(OscError::AddressInvalid);
    }
    if message.address.len() > OSC_ADDRESS_BYTES_MAX {
        return Err(OscError::AddressTooLong);
    }
    if message.args.len() > OSC_ARGS_MAX {
        return Err(OscError::ArgCountOverflow);
    }
    let mut tags = String::with_capacity(message.args.len() + 1);
    tags.push(',');
    for arg in &message.args {
        tags.push(match arg {
            OscArg::Bool(true) => 'T',
            OscArg::Bool(false) => 'F',
            OscArg::Float(_) => 'f',
            OscArg::Int(_) => 'i',
            OscArg::Str(text) => {
                if text.len() > OSC_STRING_BYTES_MAX {
                    return Err(OscError::StringTooLong);
                }
                's'
            }
        });
    }
    let mut out = Vec::with_capacity(padded(message.address.len() + 1) + padded(tags.len() + 1));
    write_string(&mut out, &message.address);
    write_string(&mut out, &tags);
    for arg in &message.args {
        match arg {
            OscArg::Bool(_) => {}
            OscArg::Float(value) => out.extend_from_slice(&value.to_be_bytes()),
            OscArg::Int(value) => out.extend_from_slice(&value.to_be_bytes()),
            OscArg::Str(text) => write_string(&mut out, text),
        }
    }
    if out.len() > OSC_PACKET_BYTES_MAX {
        return Err(OscError::PacketTooLarge);
    }
    Ok(out)
}

/// Reads one OSC string field at `cursor`: content bytes
/// up to the NUL, then padding to the 4-byte boundary.
/// Returns the text and the advanced cursor.
fn read_string(packet: &[u8], cursor: &mut usize) -> Result<String, OscError> {
    let start = *cursor;
    let mut end = start;
    while end < packet.len() && packet[end] != 0 {
        end += 1;
    }
    if end >= packet.len() {
        return Err(OscError::PacketTruncated);
    }
    let text = std::str::from_utf8(&packet[start..end]).map_err(|_| OscError::BadUtf8)?;
    let text = text.to_string();
    let next = start + padded(end - start + 1);
    if next > packet.len() {
        return Err(OscError::PacketTruncated);
    }
    *cursor = next;
    Ok(text)
}

/// Decodes one packet into one message. The packet must
/// be consumed exactly: trailing bytes refuse.
pub fn decode(packet: &[u8]) -> Result<OscMessage, OscError> {
    if packet.len() > OSC_PACKET_BYTES_MAX {
        return Err(OscError::PacketTooLarge);
    }
    if packet.is_empty() || packet.len() % 4 != 0 {
        return Err(OscError::PacketShape);
    }
    let mut cursor = 0_usize;
    let address = read_string(packet, &mut cursor)?;
    if !address.starts_with('/') {
        return Err(OscError::AddressInvalid);
    }
    if address.len() > OSC_ADDRESS_BYTES_MAX {
        return Err(OscError::AddressTooLong);
    }
    let tags = read_string(packet, &mut cursor)?;
    let Some(tag_body) = tags.strip_prefix(',') else {
        return Err(OscError::TypeTagShape);
    };
    if tag_body.len() > OSC_ARGS_MAX {
        return Err(OscError::ArgCountOverflow);
    }
    let mut args = Vec::with_capacity(tag_body.len());
    for tag in tag_body.chars() {
        let arg = match tag {
            'T' => OscArg::Bool(true),
            'F' => OscArg::Bool(false),
            'f' => {
                if cursor + 4 > packet.len() {
                    return Err(OscError::PacketTruncated);
                }
                let bytes: [u8; 4] = packet[cursor..cursor + 4]
                    .try_into()
                    .expect("four bytes checked");
                cursor += 4;
                OscArg::Float(f32::from_be_bytes(bytes))
            }
            'i' => {
                if cursor + 4 > packet.len() {
                    return Err(OscError::PacketTruncated);
                }
                let bytes: [u8; 4] = packet[cursor..cursor + 4]
                    .try_into()
                    .expect("four bytes checked");
                cursor += 4;
                OscArg::Int(i32::from_be_bytes(bytes))
            }
            's' => {
                let text = read_string(packet, &mut cursor)?;
                if text.len() > OSC_STRING_BYTES_MAX {
                    return Err(OscError::StringTooLong);
                }
                OscArg::Str(text)
            }
            other => return Err(OscError::TypeTagUnknown(other)),
        };
        args.push(arg);
    }
    if cursor != packet.len() {
        return Err(OscError::PacketShape);
    }
    Ok(OscMessage { address, args })
}

#[cfg(test)]
mod tests {
    use super::{OscArg, OscMessage, decode, encode};

    fn sample() -> OscMessage {
        OscMessage {
            address: "/VMC/Ext/Blend/Val".to_string(),
            args: vec![
                OscArg::Str("eyeBlinkLeft".to_string()),
                OscArg::Float(0.75),
                OscArg::Int(-3),
                OscArg::Bool(true),
            ],
        }
    }

    #[test]
    fn message_round_trips_byte_identical() {
        let packet = encode(&sample()).expect("encode");
        assert_eq!(packet.len() % 4, 0);
        let decoded = decode(&packet).expect("decode");
        assert_eq!(decoded, sample());
        assert_eq!(encode(&decoded).expect("re-encode"), packet);
    }

    #[test]
    fn decode_refuses_truncated_and_trailing_packets() {
        let packet = encode(&sample()).expect("encode");
        assert!(decode(&packet[..packet.len() - 4]).is_err());
        let mut trailing = packet.clone();
        trailing.extend_from_slice(&[0, 0, 0, 0]);
        assert!(decode(&trailing).is_err());
        assert!(decode(&[]).is_err());
        assert!(decode(&[0, 0, 0]).is_err());
    }

    #[test]
    fn decode_refuses_an_unknown_type_tag() {
        // Address "/x", type tags ",d" (double: not modelled).
        let mut packet = Vec::new();
        packet.extend_from_slice(b"/x\0\0");
        packet.extend_from_slice(b",d\0\0");
        packet.extend_from_slice(&[0; 8]);
        assert!(decode(&packet).is_err());
    }

    #[test]
    fn encode_refuses_bounds() {
        let long_address = OscMessage {
            address: format!("/{}", "a".repeat(300)),
            args: Vec::new(),
        };
        assert!(encode(&long_address).is_err());
        let no_slash = OscMessage {
            address: "VMC/Ext".to_string(),
            args: Vec::new(),
        };
        assert!(encode(&no_slash).is_err());
        let many_args = OscMessage {
            address: "/x".to_string(),
            args: vec![OscArg::Int(0); 65],
        };
        assert!(encode(&many_args).is_err());
    }
}
