//! ML-KEM-768 session establishment with explicit key confirmation.
//!
//! ML-KEM decapsulation uses implicit rejection, so a corrupted
//! ciphertext yields a different shared secret rather than an
//! error. The handshake therefore binds establishment to a
//! SHA-256 key-confirmation value over the shared secret and a
//! fixed transcript label; a mismatch rejects the session.

use ml_kem::kem::{Decapsulate, Encapsulate, Kem};
use ml_kem::{Ciphertext, MlKem768};
use sha2::{Digest, Sha256};

use crate::error::{BakeoffError, Result};

const CONFIRMATION_LABEL: &[u8] = b"torogao-tui-bakeoff/ml-kem-768/session-confirmation/1";

/// ML-KEM-768 ciphertext size in bytes (FIPS 203 parameter set).
pub const CIPHERTEXT_BYTES: usize = 1088;

/// ML-KEM shared-secret size in bytes (all parameter sets).
pub const SHARED_SECRET_BYTES: usize = 32;

/// The measured outcome of a valid and a tampered handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandshakeOutcome {
    /// Whether the untampered handshake established a session.
    pub valid_established: bool,
    /// Whether the tampered handshake was rejected.
    pub tamper_rejected: bool,
    /// ML-KEM ciphertext size in bytes.
    pub ciphertext_bytes: usize,
    /// Shared-secret size in bytes; the secret itself is not kept.
    pub shared_secret_bytes: usize,
}

/// Establishes one valid session and proves that a one-bit
/// ciphertext mutation is rejected by key confirmation.
pub fn establish_session() -> Result<HandshakeOutcome> {
    let valid_established = attempt(false)?;
    let tamper_rejected = !attempt(true)?;
    Ok(HandshakeOutcome {
        valid_established,
        tamper_rejected,
        ciphertext_bytes: CIPHERTEXT_BYTES,
        shared_secret_bytes: SHARED_SECRET_BYTES,
    })
}

fn attempt(tamper: bool) -> Result<bool> {
    let (decapsulation_key, encapsulation_key) = MlKem768::generate_keypair();
    let (mut ciphertext, sender_secret) = encapsulation_key.encapsulate();
    if tamper {
        tamper_first_byte(&mut ciphertext);
    }
    let receiver_secret = decapsulation_key.decapsulate(&ciphertext);
    Ok(confirmation(&sender_secret) == confirmation(&receiver_secret))
}

fn tamper_first_byte(ciphertext: &mut Ciphertext<MlKem768>) {
    assert!(
        !ciphertext.is_empty(),
        "ML-KEM-768 ciphertext is non-empty by the parameter set"
    );
    ciphertext[0] ^= 0x01;
}

fn confirmation(shared_secret: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(CONFIRMATION_LABEL);
    digest.update(shared_secret);
    digest.finalize().into()
}

/// Rejects an empty confirmation input as a programming error in
/// this prototype's fixed-size protocol.
pub fn validate_confirmation_bytes(bytes: &[u8]) -> Result<()> {
    if bytes.len() != 32 {
        return Err(BakeoffError::Crypto(format!(
            "session confirmation carries {} bytes; the bound is exactly 32",
            bytes.len()
        )));
    }
    Ok(())
}
