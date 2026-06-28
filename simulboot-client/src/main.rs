//! `simulboot-client` — the compositor.
//!
//! Two modes:
//!
//! ```text
//! # live: connect to hosts and present their surfaces in the strip
//! simulboot-client --host 127.0.0.1:7001 --host 100.0.0.2:7001 [--viewport 1440x900]
//!
//! # resume: reconstitute a suspended session from a broker URL
//! simulboot-client --resume http://100.0.0.1:7000/session/sha256:...
//! ```
//!
//! Suspend with Ctrl-C: the compositor sends `Suspend` to every host, waits for
//! `SuspendAck` (≤5s), writes the session image, and parks it on the broker so a
//! second device can resume from the printed URL.
//!
//! v0 runs headless ([`render::HeadlessRenderer`]): the strip/session/network
//! machinery is fully exercised and logged. The Metal renderer and the winit
//! event loop (real trackpad/keyboard input) plug in behind the commented
//! `wgpu`/`winit` dependencies — see [`render`].

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use simulboot_broker::ServedImage;
use tokio::sync::{mpsc, Mutex};

mod conn;
mod net;
mod render;
mod session;
mod strip;

use conn::{FrameEvent, Link};
use render::{HeadlessRenderer, Renderer};
use strip::{Strip, Surface};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let cfg = Config::from_args(std::env::args().skip(1))?;
    let endpoint = net::client_endpoint()?;

    let strip = Arc::new(Mutex::new(Strip::new(cfg.viewport.0, cfg.viewport.1)));

    // Frame-arrival pump: update per-surface diagnostics as content flows in.
    let (frame_tx, mut frame_rx) = mpsc::channel::<FrameEvent>(256);
    {
        let strip = Arc::clone(&strip);
        tokio::spawn(async move {
            while let Some(ev) = frame_rx.recv().await {
                strip.lock().await.note_frame(&ev.surface_id, ev.pts);
            }
        });
    }

    let links = match &cfg.mode {
        Mode::Live { hosts } => connect_live(&endpoint, hosts, &strip, &frame_tx).await?,
        Mode::Resume { url } => resume(&endpoint, url, &strip, &frame_tx).await?,
    };

    if links.is_empty() {
        bail!("no surfaces connected");
    }
    tracing::info!("{} surface(s) live; Ctrl-C to suspend", links.len());

    // Headless present loop.
    {
        let strip = Arc::clone(&strip);
        tokio::spawn(async move {
            let mut renderer = HeadlessRenderer::new();
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tick.tick().await;
                renderer.present(&*strip.lock().await);
            }
        });
    }

    tokio::signal::ctrl_c().await.ok();
    suspend_and_serve(links, &strip, &cfg).await
}

/// Live mode: connect to each host and append its surface to the strip.
async fn connect_live(
    endpoint: &quinn::Endpoint,
    hosts: &[SocketAddr],
    strip: &Arc<Mutex<Strip>>,
    frame_tx: &mpsc::Sender<FrameEvent>,
) -> Result<Vec<Link>> {
    let mut links = Vec::new();
    for &addr in hosts {
        let link = conn::connect(endpoint, addr, None, frame_tx.clone())
            .await
            .with_context(|| format!("connecting to host {addr}"))?;
        let mut s = strip.lock().await;
        let surface = surface_from_announce(&link.announce, addr, &s);
        s.append(surface);
        tracing::info!(name = %link.announce.name, %addr, "surface added");
        drop(s);
        links.push(link);
    }
    Ok(links)
}

/// Resume mode: fetch the session image, reconnect each surface's host, and
/// restore order, scroll position, and focus (F4).
async fn resume(
    endpoint: &quinn::Endpoint,
    url: &str,
    strip: &Arc<Mutex<Strip>>,
    frame_tx: &mpsc::Sender<FrameEvent>,
) -> Result<Vec<Link>> {
    tracing::info!(%url, "resuming session");
    let image = session::fetch_image(url).await?;
    let session_id = image.id.clone().unwrap_or_else(|| image.compute_id());
    tracing::info!(id = %session_id, surfaces = image.surfaces.len(), "session image loaded");

    let mut links = Vec::new();
    for entry in &image.surfaces {
        let addr: SocketAddr = entry
            .host
            .address
            .parse()
            .with_context(|| format!("bad host address {:?}", entry.host.address))?;
        let link = conn::connect(endpoint, addr, Some(session_id.clone()), frame_tx.clone())
            .await
            .with_context(|| format!("reconnecting host {addr}"))?;

        let mut s = strip.lock().await;
        let mut surface = surface_from_announce(&link.announce, addr, &s);
        // The image is authoritative for placement.
        surface.order = entry.order;
        surface.width = s.default_surface_width();
        surface.height = s.viewport_height();
        s.insert_ordered(surface);
        drop(s);
        links.push(link);
    }

    // Restore layout once every surface is back.
    let mut s = strip.lock().await;
    s.set_scroll(image.layout.scroll_pos);
    s.set_focus_opt(image.layout.focus);
    Ok(links)
}

/// Build a strip [`Surface`] from a host announce.
fn surface_from_announce(
    announce: &simulboot_common::SurfaceAnnounce,
    addr: SocketAddr,
    strip: &Strip,
) -> Surface {
    Surface {
        id: announce.id,
        name: announce.name.clone(),
        order: 0,
        width: strip.default_surface_width(),
        height: strip.viewport_height(),
        source_width: announce.width,
        source_height: announce.height,
        codec: announce.codec,
        provenance: announce.provenance.clone(),
        host_addr: addr,
        last_pts: None,
        frames_received: 0,
    }
}

/// The suspension flow: Suspend → SuspendAck → checkpoint → serve (F3, brief
/// "Session checkpoint" steps 1–7).
async fn suspend_and_serve(links: Vec<Link>, strip: &Arc<Mutex<Strip>>, cfg: &Config) -> Result<()> {
    tracing::info!("suspending: notifying {} host(s)", links.len());
    let acks = futures_join_all(links.iter().map(|l| l.suspend(Duration::from_secs(5)))).await;
    let acked = acks.iter().filter(|ok| **ok).count();
    tracing::info!("{acked}/{} hosts acknowledged suspend", links.len());

    let image = {
        let s = strip.lock().await;
        session::image_from_strip(&s, session::now_rfc3339())
    };
    let id = image.id.clone().unwrap_or_else(|| image.compute_id());
    let xml = image.to_xml();

    std::fs::write(&cfg.out, &xml)
        .with_context(|| format!("writing session image to {}", cfg.out))?;
    tracing::info!("wrote session image to {}", cfg.out);

    // Hosts keep running (F5); they will reconnect to whichever compositor
    // resumes from the served image.
    println!(
        "\nSession suspended. Resume at:\n  http://<this-tailscale-ip>:{}/session/{id}\n\
         (serving {}; Ctrl-C to stop)",
        cfg.broker_port, cfg.out
    );

    let served = ServedImage::new_xml(id, xml.into_bytes());
    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
    };
    simulboot_broker::serve(("0.0.0.0", cfg.broker_port), served, shutdown).await
}

/// Minimal `join_all` so we don't pull in `futures` just for this.
async fn futures_join_all<F, T>(futs: impl IntoIterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T>,
{
    let mut out = Vec::new();
    for f in futs {
        out.push(f.await);
    }
    out
}

/// Parsed CLI configuration.
struct Config {
    mode: Mode,
    viewport: (f32, f32),
    out: String,
    broker_port: u16,
}

enum Mode {
    Live { hosts: Vec<SocketAddr> },
    Resume { url: String },
}

impl Config {
    fn from_args(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut hosts = Vec::new();
        let mut resume = None;
        let mut viewport = (1440.0, 900.0);
        let mut out = "session.xml".to_string();
        let mut broker_port = 7000u16;

        let mut args = args;
        while let Some(arg) = args.next() {
            let mut next = |flag: &str| args.next().with_context(|| format!("{flag} needs a value"));
            match arg.as_str() {
                "--host" => hosts.push(
                    next("--host")?
                        .parse()
                        .context("--host must be host:port")?,
                ),
                "--resume" => resume = Some(next("--resume")?),
                "--viewport" => viewport = parse_viewport(&next("--viewport")?)?,
                "--out" => out = next("--out")?,
                "--broker-port" => {
                    broker_port = next("--broker-port")?
                        .parse()
                        .context("--broker-port must be a number")?
                }
                "-h" | "--help" => {
                    eprintln!(
                        "usage:\n  simulboot-client --host H:P [--host H:P ...] \
                         [--viewport 1440x900] [--out session.xml] [--broker-port 7000]\n  \
                         simulboot-client --resume http://HOST:7000/session/ID"
                    );
                    std::process::exit(0);
                }
                other => bail!("unknown argument: {other}"),
            }
        }

        let mode = match resume {
            Some(url) => {
                if !hosts.is_empty() {
                    bail!("--resume cannot be combined with --host");
                }
                Mode::Resume { url }
            }
            None => {
                if hosts.is_empty() {
                    bail!("provide --host H:P (one per surface) or --resume URL");
                }
                Mode::Live { hosts }
            }
        };

        Ok(Config { mode, viewport, out, broker_port })
    }
}

fn parse_viewport(s: &str) -> Result<(f32, f32)> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .context("--viewport must look like 1440x900")?;
    Ok((w.parse().context("bad viewport width")?, h.parse().context("bad viewport height")?))
}
