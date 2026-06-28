//! Shared types for simulboot.
//!
//! This crate is the single source of truth shared by every other crate in the
//! workspace. It contains two things, and nothing platform-specific:
//!
//! * [`wire`] — the QUIC wire protocol: the [`wire::StructureMessage`] control
//!   enum exchanged on the reliable stream, and the [`wire::FrameHeader`] that
//!   prefixes encoded frames on the datagram channel. Both are serialised with
//!   bincode behind a length prefix (see [`wire::encode_frame`]).
//!
//! * [`session`] — the session image: a self-describing snapshot of a running
//!   session (its surfaces, their provenance, and the strip layout) rendered to
//!   an XML document and content-addressed by the SHA-256 of its canonical form.
//!
//! Per the v0 brief this crate deliberately avoids: Cap'n Proto (bincode for
//! now), session types (plain enums), and Fast Infoset (text XML for now).

pub mod session;
pub mod wire;

// Re-export the most commonly used items at the crate root so downstream crates
// can `use simulboot_common::{StructureMessage, SurfaceId, ...}`.
pub use session::{HostEntry, Layout, SessionError, SessionImage, SurfaceEntry};
pub use wire::{
    Codec, FrameHeader, HostProvenance, InputEvent, OsKind, StructureMessage, SurfaceAnnounce,
    surface_id_from_seed, SurfaceId, WireError, ALPN,
};
