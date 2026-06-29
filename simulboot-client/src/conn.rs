//! Per-host QUIC connection management (the compositor side).
//!
//! For each host the compositor opens a [`Link`]: it dials the host, reads the
//! host's [`StructureMessage::Announce`] on the structure stream, then runs three
//! concurrent tasks — a writer (control/input out), a reader (control in: acks,
//! reconnect results), and a datagram pump (content frames in).
//!
//! The strip and session logic only ever touch [`Link`]: the control sender, the
//! announce, and a suspend-ack signal. Everything OS-specific stayed on the host.
//!
//! `Link::send_input` and `Link::host_addr` are part of the link API the winit
//! input layer (build-order week 6) will drive; the headless v0 path does not
//! yet call them.
#![allow(dead_code)]

use std::io::Cursor;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use quinn::Endpoint;
use simulboot_common::wire::{decode_frame, encode_frame, WireError};
use simulboot_common::{FrameHeader, StructureMessage, SurfaceAnnounce, SurfaceId};

/// A frame arrival, surfaced to the compositor for diagnostics/repaint.
#[derive(Debug, Clone, Copy)]
pub struct FrameEvent {
    pub surface_id: SurfaceId,
    pub pts: u64,
}

/// A live connection to one host.
pub struct Link {
    /// The surface this host announced (its current identity).
    pub announce: SurfaceAnnounce,
    /// Where this host lives, recorded into the session image.
    pub host_addr: SocketAddr,
    /// Send control/input messages to the host (input, suspend, disconnect).
    pub control: async_channel::Sender<StructureMessage>,
    /// Receives a single message once the host has acknowledged a `Suspend`.
    pub suspend_ack: async_channel::Receiver<()>,
}

impl Link {
    /// Forward a normalised input event to the host's focused surface.
    pub async fn send_input(&self, event: simulboot_common::InputEvent) -> Result<()> {
        self.control
            .send(StructureMessage::InputEvent { surface_id: self.announce.id, event })
            .await
            .context("host control channel closed")?;
        Ok(())
    }

    /// Ask the host to suspend; resolves when `SuspendAck` is observed or `within`
    /// elapses. The host stays alive either way (F5).
    pub async fn suspend(&self, within: Duration) -> bool {
        if self.control.send(StructureMessage::Suspend).await.is_err() {
            return false;
        }
        smol::future::or(
            async { self.suspend_ack.recv().await.is_ok() },
            async {
                smol::Timer::after(within).await;
                false
            },
        )
        .await
    }
}

/// Dial `addr` and bring up a [`Link`]. If `resume_session_id` is set, the
/// compositor additionally sends `Reconnect` after reading the announce (resume
/// flow); the host replies `ReconnectOk` which the reader task absorbs.
pub async fn connect(
    endpoint: &Endpoint,
    addr: SocketAddr,
    resume_session_id: Option<String>,
    frames: async_channel::Sender<FrameEvent>,
) -> Result<Link> {
    let connection = endpoint
        .connect(addr, "simulboot-host")
        .context("starting QUIC connection")?
        .await
        .with_context(|| format!("connecting to host {addr}"))?;

    // The host opens the structure stream and announces first.
    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .context("accepting structure stream")?;

    let mut buf = Vec::with_capacity(4096);
    let announce = match read_message(&mut recv, &mut buf).await? {
        Some(StructureMessage::Announce(a)) => a,
        Some(StructureMessage::ReconnectOk(a)) => a,
        Some(other) => bail!("expected Announce from host, got {other:?}"),
        None => bail!("host closed structure stream before announcing"),
    };

    let (control_tx, control_rx) = async_channel::bounded::<StructureMessage>(64);
    let (ack_tx, ack_rx) = async_channel::bounded::<()>(1);

    // Resume: nudge the host to reconnect this surface into the session.
    if let Some(session_id) = resume_session_id {
        let frame = encode_frame(&StructureMessage::Reconnect { session_id })?;
        send.write_all(&frame).await.context("sending Reconnect")?;
    }

    // Writer task: drains control/input out to the host.
    smol::spawn(async move {
        while let Ok(msg) = control_rx.recv().await {
            let bytes = match encode_frame(&msg) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, "encoding control message");
                    continue;
                }
            };
            if let Err(e) = send.write_all(&bytes).await {
                tracing::warn!(error = %e, "host write failed; closing writer");
                break;
            }
            if matches!(msg, StructureMessage::Disconnect) {
                let _ = send.finish();
                break;
            }
        }
    })
    .detach();

    // Reader task: absorbs acks and reconnect results.
    smol::spawn(async move {
        loop {
            match read_message(&mut recv, &mut buf).await {
                Ok(Some(StructureMessage::SuspendAck)) => {
                    let _ = ack_tx.try_send(());
                }
                Ok(Some(StructureMessage::ReconnectOk(a))) => {
                    tracing::info!(name = %a.name, "host reconnected");
                }
                Ok(Some(StructureMessage::ReconnectFail { reason })) => {
                    tracing::warn!(%reason, "host failed to reconnect");
                }
                Ok(Some(other)) => tracing::debug!(?other, "ignoring host message"),
                Ok(None) => {
                    tracing::info!("host structure stream closed");
                    break;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "host structure read error");
                    break;
                }
            }
        }
    })
    .detach();

    // Datagram pump: content frames in.
    let dgram_conn = connection.clone();
    smol::spawn(async move {
        loop {
            match dgram_conn.read_datagram().await {
                Ok(datagram) => match decode_datagram(&datagram) {
                    Ok(ev) => {
                        if frames.send(ev).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "bad content datagram"),
                },
                Err(e) => {
                    tracing::info!(error = %e, "content channel closed");
                    break;
                }
            }
        }
    })
    .detach();

    Ok(Link { announce, host_addr: addr, control: control_tx, suspend_ack: ack_rx })
}

/// Decode a content datagram: a bincode [`FrameHeader`] followed by the encoded
/// frame bytes.
fn decode_datagram(datagram: &[u8]) -> Result<FrameEvent> {
    let mut cursor = Cursor::new(datagram);
    let header: FrameHeader =
        bincode::deserialize_from(&mut cursor).context("decoding frame header")?;
    let offset = cursor.position() as usize;
    let payload = &datagram[offset..];
    if payload.len() != header.len as usize {
        tracing::debug!(
            declared = header.len,
            actual = payload.len(),
            "frame length mismatch"
        );
    }
    // The decoded pixels would be uploaded to the renderer here; v0 only tracks
    // arrival metadata.
    Ok(FrameEvent { surface_id: header.surface_id, pts: header.pts })
}

/// Read one length-prefixed [`StructureMessage`], buffering partial reads.
async fn read_message(
    recv: &mut quinn::RecvStream,
    buf: &mut Vec<u8>,
) -> Result<Option<StructureMessage>> {
    loop {
        match decode_frame::<StructureMessage>(buf) {
            Ok((msg, consumed)) => {
                buf.drain(..consumed);
                return Ok(Some(msg));
            }
            Err(WireError::Incomplete { .. }) => {}
            Err(e) => return Err(e.into()),
        }
        let mut tmp = [0u8; 4096];
        match recv.read(&mut tmp).await.context("reading structure stream")? {
            Some(0) | None => {
                return if buf.is_empty() {
                    Ok(None)
                } else {
                    bail!("stream ended mid-frame")
                };
            }
            Some(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
}
