//! `simulboot-host` — capture a surface on a source machine and serve it to the
//! compositor over QUIC.
//!
//! ```text
//! simulboot-host [--bind 0.0.0.0:7001] [--name macOS] [--os macos|windows|linux]
//!                [--machine NAME] [--capture window:Safari]
//!                [--tailscale-addr ADDR] [--width 960] [--height 540]
//! ```
//!
//! The host is the QUIC server; the compositor dials in. On each connection the
//! host opens the structure stream, sends a [`StructureMessage::Announce`], then
//! pumps encoded frames as datagrams while routing inbound input to the capture
//! backend. The host *persists* across session suspension (F5): on `Suspend` it
//! replies `SuspendAck` and keeps running, ready for a future reconnect.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use bytes::Bytes;
use simulboot_common::wire::{decode_frame, WireError};
use simulboot_common::{
    surface_id_from_seed, Codec, FrameHeader, HostProvenance, OsKind, StructureMessage,
    SurfaceAnnounce,
};
use tokio::sync::mpsc;

mod capture;
mod net;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

use capture::{CaptureSource, EncodedFrame};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // rustls 0.23 needs a process-wide crypto provider before any TLS config.
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let cfg = Config::from_args(std::env::args().skip(1))?;
    let announce = cfg.surface_announce();

    let endpoint = net::server_endpoint(cfg.bind)?;
    tracing::info!(bind = %cfg.bind, name = %cfg.name, "host listening");

    while let Some(incoming) = endpoint.accept().await {
        let announce = announce.clone();
        let os = cfg.os;
        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    let peer = conn.remote_address();
                    tracing::info!(%peer, "compositor connected");
                    if let Err(e) = serve_connection(conn, announce, os).await {
                        tracing::warn!(%peer, error = %e, "connection ended");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "handshake failed"),
            }
        });
    }

    Ok(())
}

/// Drive one compositor connection: announce, then concurrently pump frames out
/// and route input/control messages in.
async fn serve_connection(conn: quinn::Connection, announce: SurfaceAnnounce, os: OsKind) -> Result<()> {
    // Structure channel: a bidirectional stream the host opens to announce.
    let (mut send, mut recv) = conn.open_bi().await.context("opening structure stream")?;
    send_message(&mut send, &StructureMessage::Announce(announce.clone()))
        .await
        .context("sending Announce")?;

    // Wire the capture backend to the runtime via channels.
    let (frame_tx, mut frame_rx) = mpsc::channel::<EncodedFrame>(8);
    let (input_tx, input_rx) = mpsc::channel(64);
    let source = build_source(os, announce.clone())?;
    let capture_task = tokio::task::spawn_blocking(move || source.run(frame_tx, input_rx));

    // Outbound: forward encoded frames onto the content (datagram) channel.
    let surface_id = announce.id;
    let conn_out = conn.clone();
    let frame_task = tokio::spawn(async move {
        while let Some(frame) = frame_rx.recv().await {
            if let Err(e) = send_frame(&conn_out, surface_id, &frame) {
                tracing::warn!(error = %e, "dropping frame");
            }
        }
    });

    // Inbound: structure-channel control loop.
    let mut buf = Vec::with_capacity(4096);
    loop {
        match read_message(&mut recv, &mut buf).await? {
            None => {
                tracing::info!("structure stream closed by compositor");
                break;
            }
            Some(StructureMessage::InputEvent { surface_id: sid, event }) => {
                if sid == surface_id {
                    // Best-effort: if the backend is gone, stop routing.
                    if input_tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
            Some(StructureMessage::Resize { width, height, .. }) => {
                tracing::info!(width, height, "resize requested (capture is fixed-size in v0)");
            }
            Some(StructureMessage::Suspend) => {
                tracing::info!("suspend requested; acking and staying alive");
                send_message(&mut send, &StructureMessage::SuspendAck).await?;
            }
            Some(StructureMessage::Reconnect { session_id }) => {
                // A reconnect normally arrives on a *fresh* connection after the
                // session resumes elsewhere; handling it here too keeps the host
                // robust if the compositor reuses the link.
                tracing::info!(%session_id, "reconnect on existing connection; re-announcing");
                send_message(&mut send, &StructureMessage::ReconnectOk(announce.clone())).await?;
            }
            Some(StructureMessage::Disconnect) => {
                tracing::info!("compositor requested disconnect");
                break;
            }
            Some(other) => {
                tracing::debug!(?other, "ignoring host→compositor message received from compositor");
            }
        }
    }

    // Tear down: closing input_tx ends the capture loop, which ends frame_rx.
    drop(input_tx);
    frame_task.abort();
    let _ = capture_task.await;
    Ok(())
}

/// Select the platform capture backend. Falls back to [`NullCapture`] on
/// platforms without an implemented backend.
fn build_source(os: OsKind, announce: SurfaceAnnounce) -> Result<Box<dyn CaptureSource>> {
    let _ = os;
    #[cfg(target_os = "macos")]
    {
        return macos::build_source(announce);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::build_source(announce);
    }
    #[cfg(target_os = "linux")]
    {
        return linux::build_source(announce);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        tracing::warn!("no platform backend for this OS; using NullCapture");
        Ok(Box::new(capture::NullCapture::new(announce)))
    }
}

/// Serialise and send one [`StructureMessage`] as a length-prefixed frame.
async fn send_message(send: &mut quinn::SendStream, msg: &StructureMessage) -> Result<()> {
    let frame = simulboot_common::wire::encode_frame(msg)?;
    send.write_all(&frame).await.context("writing structure frame")?;
    Ok(())
}

/// Encode `frame` with its [`FrameHeader`] and send it as a single datagram on
/// the content channel.
///
/// v0 sends one frame per datagram. QUIC datagrams are MTU-bounded, so frames
/// larger than the path's `max_datagram_size` are dropped with a warning;
/// fragmentation is a later concern (and irrelevant while backends are stubs).
fn send_frame(conn: &quinn::Connection, surface_id: [u8; 32], frame: &EncodedFrame) -> Result<()> {
    let header = FrameHeader { surface_id, pts: frame.pts, len: frame.bytes.len() as u32 };
    let mut buf = bincode::serialize(&header).context("serialising frame header")?;
    buf.extend_from_slice(&frame.bytes);

    if let Some(max) = conn.max_datagram_size() {
        if buf.len() > max {
            tracing::warn!(len = buf.len(), max, "frame exceeds datagram size; dropping (TODO: fragment)");
            return Ok(());
        }
    }
    conn.send_datagram(Bytes::from(buf)).context("sending datagram")?;
    Ok(())
}

/// Read one length-prefixed [`StructureMessage`] from `recv`, buffering partial
/// reads in `buf`. Returns `Ok(None)` at clean end-of-stream.
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
            Err(WireError::Incomplete { .. }) => {} // need more bytes
            Err(e) => return Err(e.into()),
        }
        let mut tmp = [0u8; 4096];
        match recv.read(&mut tmp).await.context("reading structure stream")? {
            Some(0) | None => {
                return if buf.is_empty() {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("stream ended mid-frame"))
                };
            }
            Some(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
}

/// Parsed command-line configuration.
struct Config {
    bind: SocketAddr,
    name: String,
    os: OsKind,
    machine: String,
    capture: String,
    tailscale_addr: String,
    width: u32,
    height: u32,
}

impl Config {
    fn from_args(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut bind: SocketAddr = "0.0.0.0:7001".parse().unwrap();
        let mut name = "surface".to_string();
        let mut os = default_os();
        let mut machine = hostname();
        let mut capture = "display:0".to_string();
        let mut tailscale_addr = String::new();
        let mut width = 960u32;
        let mut height = 540u32;

        let mut args = args;
        while let Some(arg) = args.next() {
            let mut next = |flag: &str| args.next().with_context(|| format!("{flag} needs a value"));
            match arg.as_str() {
                "--bind" => bind = next("--bind")?.parse().context("--bind must be host:port")?,
                "--name" => name = next("--name")?,
                "--os" => os = parse_os(&next("--os")?)?,
                "--machine" => machine = next("--machine")?,
                "--capture" => capture = next("--capture")?,
                "--tailscale-addr" => tailscale_addr = next("--tailscale-addr")?,
                "--width" => width = next("--width")?.parse().context("--width must be a number")?,
                "--height" => height = next("--height")?.parse().context("--height")?,
                "-h" | "--help" => {
                    eprintln!(
                        "usage: simulboot-host [--bind 0.0.0.0:7001] [--name NAME] \
                         [--os macos|windows|linux] [--machine NAME] [--capture window:Safari] \
                         [--tailscale-addr ADDR] [--width 960] [--height 540]"
                    );
                    std::process::exit(0);
                }
                other => anyhow::bail!("unknown argument: {other}"),
            }
        }

        if tailscale_addr.is_empty() {
            tailscale_addr = bind.ip().to_string();
        }

        Ok(Config { bind, name, os, machine, capture, tailscale_addr, width, height })
    }

    fn surface_announce(&self) -> SurfaceAnnounce {
        let seed = format!("{}|{}|{}", self.machine, self.capture, self.name);
        SurfaceAnnounce {
            id: surface_id_from_seed(&seed),
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            codec: Codec::H265,
            provenance: HostProvenance {
                os: self.os,
                machine_name: self.machine.clone(),
                tailscale_addr: self.tailscale_addr.clone(),
                capture_description: self.capture.clone(),
            },
        }
    }
}

fn parse_os(s: &str) -> Result<OsKind> {
    match s.to_ascii_lowercase().as_str() {
        "macos" | "mac" | "osx" => Ok(OsKind::MacOS),
        "windows" | "win" => Ok(OsKind::Windows),
        "linux" => Ok(OsKind::Linux),
        other => anyhow::bail!("unknown os: {other} (expected macos|windows|linux)"),
    }
}

fn default_os() -> OsKind {
    #[cfg(target_os = "macos")]
    {
        OsKind::MacOS
    }
    #[cfg(target_os = "windows")]
    {
        OsKind::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        OsKind::Linux
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string())
}
