//! The simulboot wire protocol.
//!
//! Two channels run between every host and the compositor:
//!
//! * **Structure channel** — a QUIC reliable stream carrying [`StructureMessage`]
//!   control messages (announcements, input, suspend/reconnect handshakes).
//! * **Content channel** — QUIC datagrams, each a [`FrameHeader`] immediately
//!   followed by `header.len` bytes of the encoded video frame.
//!
//! Both directions use the same framing: a 4-byte big-endian length prefix
//! followed by a bincode-serialised body. Use [`encode_frame`] to produce a
//! frame and [`decode_frame`] to pull one back out of a buffer.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// QUIC ALPN protocol identifier negotiated between hosts and the compositor.
/// Bump the version suffix on any breaking change to the wire protocol.
pub const ALPN: &[u8] = b"simulboot/0";

/// Stable identifier for a surface: the SHA-256 of the content that defines it.
///
/// Hosts mint this when they announce a surface; it is the key the compositor
/// uses everywhere (strip ordering, input routing, focus) and it is what gets
/// written into the session image so the surface can be re-identified on resume.
pub type SurfaceId = [u8; 32];

/// Mint a [`SurfaceId`] from a stable seed (e.g. machine name + capture target).
///
/// Hosts use this so the same surface gets the same id across reconnects, which
/// is what lets a resumed session re-bind a reconnecting host to its strip slot.
pub fn surface_id_from_seed(seed: &str) -> SurfaceId {
    Sha256::digest(seed.as_bytes()).into()
}

/// Video codec carried on the content channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Codec {
    H265,
    AV1,
}

/// Which operating system produced a surface. Recorded for provenance only —
/// the compositor treats every surface identically regardless of this value
/// (Claim B: surface uniformity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OsKind {
    MacOS,
    Windows,
    Linux,
}

/// Where a surface came from. Queryable provenance (F8): which host, which OS,
/// which machine, and how it was captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProvenance {
    pub os: OsKind,
    pub machine_name: String,
    pub tailscale_addr: String,
    /// e.g. "window:Safari", "display:0", "vm:PhysicalDrive1".
    pub capture_description: String,
}

/// A host's declaration that a new surface exists. Sent host → compositor on the
/// structure channel, both at first connection and on reconnect after resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceAnnounce {
    pub id: SurfaceId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub codec: Codec,
    pub provenance: HostProvenance,
}

/// Prepended to every encoded frame on the content channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameHeader {
    pub surface_id: SurfaceId,
    pub pts: u64,
    /// Byte length of the encoded frame immediately following this header.
    pub len: u32,
}

/// A normalised input event. Coordinates are device-independent: the compositor
/// produces them in surface-local 0.0–1.0 space (F15) and the host denormalises
/// for injection into the source OS.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum InputEvent {
    KeyDown { keycode: u32 },
    KeyUp { keycode: u32 },
    /// Normalised 0.0–1.0 within the surface's bounds.
    MouseMove { x: f32, y: f32 },
    MouseDown { button: u8 },
    MouseUp { button: u8 },
    Scroll { dx: f32, dy: f32 },
}

/// Every control message on the structure channel. A plain enum stands in for
/// the Scribble-specified session type for v0 (see the handoff brief).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StructureMessage {
    // ---- Host → Compositor ----
    Announce(SurfaceAnnounce),
    SuspendAck,
    ReconnectOk(SurfaceAnnounce),
    ReconnectFail { reason: String },

    // ---- Compositor → Host ----
    InputEvent { surface_id: SurfaceId, event: InputEvent },
    Resize { surface_id: SurfaceId, width: u32, height: u32 },
    Suspend,
    Reconnect { session_id: String },
    Disconnect,
}

/// Errors from framing/serialisation of wire messages.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("bincode (de)serialisation failed: {0}")]
    Bincode(#[from] bincode::Error),
    /// The buffer did not contain a full 4-byte length prefix yet.
    #[error("incomplete frame: need {needed} bytes, have {have}")]
    Incomplete { needed: usize, have: usize },
    /// A frame claimed a body larger than the configured maximum.
    #[error("frame too large: {len} bytes exceeds limit of {limit}")]
    TooLarge { len: usize, limit: usize },
}

/// Upper bound on a single structure-channel frame body. Control messages are
/// tiny; this only exists to stop a malformed length prefix from triggering a
/// huge allocation.
pub const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

/// Serialise `msg` to a length-prefixed bincode frame: a 4-byte big-endian
/// length followed by the body.
pub fn encode_frame<T: Serialize>(msg: &T) -> Result<Vec<u8>, WireError> {
    let body = bincode::serialize(msg)?;
    let mut buf = Vec::with_capacity(body.len() + 4);
    buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(buf)
}

/// Try to decode one frame from the front of `buf`.
///
/// On success returns the decoded message and the number of bytes consumed, so
/// the caller can drain its receive buffer (`buf.drain(..consumed)`). Returns
/// [`WireError::Incomplete`] when more bytes are needed — the caller should read
/// more and retry without discarding anything.
pub fn decode_frame<T: DeserializeOwned>(buf: &[u8]) -> Result<(T, usize), WireError> {
    if buf.len() < 4 {
        return Err(WireError::Incomplete { needed: 4, have: buf.len() });
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > MAX_FRAME_LEN {
        return Err(WireError::TooLarge { len, limit: MAX_FRAME_LEN });
    }
    let total = 4 + len;
    if buf.len() < total {
        return Err(WireError::Incomplete { needed: total, have: buf.len() });
    }
    let msg = bincode::deserialize(&buf[4..total])?;
    Ok((msg, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_announce() -> SurfaceAnnounce {
        SurfaceAnnounce {
            id: [7u8; 32],
            name: "macOS".to_string(),
            width: 960,
            height: 540,
            codec: Codec::H265,
            provenance: HostProvenance {
                os: OsKind::MacOS,
                machine_name: "Aleth-MacBook".to_string(),
                tailscale_addr: "127.0.0.1".to_string(),
                capture_description: "window:Safari".to_string(),
            },
        }
    }

    #[test]
    fn frame_roundtrip() {
        let msg = StructureMessage::Announce(sample_announce());
        let bytes = encode_frame(&msg).unwrap();
        let (decoded, consumed): (StructureMessage, usize) = decode_frame(&bytes).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(consumed, bytes.len());
    }

    #[test]
    fn decode_reports_incomplete_prefix() {
        let err = decode_frame::<StructureMessage>(&[0, 0]).unwrap_err();
        assert!(matches!(err, WireError::Incomplete { needed: 4, .. }));
    }

    #[test]
    fn decode_reports_incomplete_body() {
        let bytes = encode_frame(&StructureMessage::Suspend).unwrap();
        let err = decode_frame::<StructureMessage>(&bytes[..bytes.len() - 1]).unwrap_err();
        assert!(matches!(err, WireError::Incomplete { .. }));
    }

    #[test]
    fn two_frames_back_to_back() {
        let a = encode_frame(&StructureMessage::Suspend).unwrap();
        let b = encode_frame(&StructureMessage::SuspendAck).unwrap();
        let mut stream = a.clone();
        stream.extend_from_slice(&b);

        let (m1, c1): (StructureMessage, usize) = decode_frame(&stream).unwrap();
        assert_eq!(m1, StructureMessage::Suspend);
        let (m2, c2): (StructureMessage, usize) = decode_frame(&stream[c1..]).unwrap();
        assert_eq!(m2, StructureMessage::SuspendAck);
        assert_eq!(c1 + c2, stream.len());
    }

    #[test]
    fn frame_header_roundtrip() {
        let h = FrameHeader { surface_id: [1u8; 32], pts: 123456, len: 4096 };
        let bytes = encode_frame(&h).unwrap();
        let (decoded, _): (FrameHeader, usize) = decode_frame(&bytes).unwrap();
        assert_eq!(decoded, h);
    }
}
