//! Deterministic handshake inputs shared by encrypted test peers.

pub const INITIATOR_RANDOM: [u8; 32] = [1; 32];
pub const INITIATOR_SECRET: [u8; 32] = [3; 32];
pub const RESPONDER_RANDOM: [u8; 32] = [2; 32];
pub const RESPONDER_SECRET: [u8; 32] = [4; 32];
