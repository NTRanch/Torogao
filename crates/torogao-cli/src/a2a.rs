//! The A2A HTTP surface. Ported after the core tool
//! family; see a2a/server.d in the reference tree for
//! the wire contract.

/// The default A2A port.
pub const A2A_DEFAULT_PORT: u16 = 8377;

/// Serves A2A over HTTP until killed; returns the exit
/// code.
pub fn run_a2a_server(_bind: &str, _port: u16) -> i32 {
    -1
}
