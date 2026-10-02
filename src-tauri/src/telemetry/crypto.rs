//! GT7 "Simulator Interface" packet encryption.
//!
//! Packets are Salsa20-encrypted with a fixed key. The 8-byte nonce is derived
//! from a 32-bit IV stored (unencrypted) at offset 0x40 of every packet, XORed
//! with a constant that depends on which heartbeat variant was requested.

use salsa20::cipher::{KeyIvInit, StreamCipher};
use salsa20::Salsa20;

const KEY: &[u8; 32] = b"Simulator Interface Packet GT7 v";
pub const MAGIC: u32 = 0x4737_5330; // "0S7G"

/// Which packet format the PlayStation sends, selected by the heartbeat byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Variant {
    /// Original format, 296 bytes.
    A,
    /// Adds wheel rotation + sway/heave/surge, 316 bytes.
    B,
    /// Adds raw driver inputs and energy recovery, 344 bytes.
    Tilde,
}

impl Variant {
    pub const ALL: [Variant; 3] = [Variant::Tilde, Variant::B, Variant::A];

    pub fn heartbeat(self) -> &'static [u8] {
        match self {
            Variant::A => b"A",
            Variant::B => b"B",
            Variant::Tilde => b"~",
        }
    }

    pub fn xor(self) -> u32 {
        match self {
            Variant::A => 0xDEAD_BEAF,
            Variant::B => 0xDEAD_BEEF,
            Variant::Tilde => 0x55FA_BB4F,
        }
    }

    pub fn size(self) -> usize {
        match self {
            Variant::A => 0x128,
            Variant::B => 0x13C,
            Variant::Tilde => 0x158,
        }
    }

    pub fn from_size(len: usize) -> Option<Variant> {
        Variant::ALL.into_iter().find(|v| v.size() == len)
    }
}

fn apply(buf: &mut [u8], iv1: u32, variant: Variant) {
    let iv2 = iv1 ^ variant.xor();
    let mut nonce = [0u8; 8];
    nonce[..4].copy_from_slice(&iv2.to_le_bytes());
    nonce[4..].copy_from_slice(&iv1.to_le_bytes());
    let mut cipher = Salsa20::new(KEY.into(), &nonce.into());
    cipher.apply_keystream(buf);
}

fn magic_ok(buf: &[u8]) -> bool {
    buf.len() >= 4 && u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) == MAGIC
}

/// Decrypt a raw UDP datagram. Returns the plaintext and the detected variant,
/// or `None` if it is not a valid GT7 packet.
pub fn decrypt(raw: &[u8]) -> Option<(Vec<u8>, Variant)> {
    if raw.len() < 0x128 {
        return None;
    }
    let iv1 = u32::from_le_bytes(raw[0x40..0x44].try_into().ok()?);
    // Try the variant implied by the length first, then the rest.
    let first = Variant::from_size(raw.len());
    let order = first
        .into_iter()
        .chain(Variant::ALL.into_iter().filter(move |v| Some(*v) != first));
    for variant in order {
        let mut buf = raw.to_vec();
        apply(&mut buf, iv1, variant);
        if magic_ok(&buf) {
            return Some((buf, variant));
        }
    }
    None
}

/// Encrypt a plaintext packet the way the PlayStation does. Used by the demo
/// simulator and tests.
pub fn encrypt(plain: &[u8], iv1: u32, variant: Variant) -> Vec<u8> {
    let mut buf = plain.to_vec();
    apply(&mut buf, iv1, variant);
    buf[0x40..0x44].copy_from_slice(&iv1.to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_first_32_bytes_of_spec_string() {
        assert_eq!(&KEY[..], &b"Simulator Interface Packet GT7 ver 0.0"[..32]);
    }

    #[test]
    fn round_trip_all_variants() {
        for v in Variant::ALL {
            let mut plain = vec![0u8; v.size()];
            plain[..4].copy_from_slice(&MAGIC.to_le_bytes());
            plain[0x3C..0x40].copy_from_slice(&7000f32.to_le_bytes());
            let enc = encrypt(&plain, 0x1234_5678, v);
            assert_ne!(enc[..4], plain[..4]);
            let (dec, dv) = decrypt(&enc).expect("decrypts");
            assert_eq!(dv, v);
            assert_eq!(&dec[0x3C..0x40], &plain[0x3C..0x40]);
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(decrypt(&vec![0xAB; 0x128]).is_none());
        assert!(decrypt(b"A").is_none());
    }
}
