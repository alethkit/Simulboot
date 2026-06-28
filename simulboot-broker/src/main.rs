//! `simulboot-broker` — serve a session image file over HTTP.
//!
//! ```text
//! simulboot-broker <session-image.xml> [--bind 0.0.0.0] [--port 7000]
//! ```
//!
//! Reads the XML session image, derives its content id, and serves it at
//! `GET /session/{id}` until interrupted (Ctrl-C). In the demo this role is
//! usually played by the suspending compositor itself (which links this crate's
//! [`simulboot_broker::serve`]); the standalone binary is for testing and for
//! parking an image independently of any compositor.

use std::process::ExitCode;

use anyhow::{bail, Context};
use simulboot_broker::{serve, ServedImage};
use simulboot_common::SessionImage;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let cfg = Config::from_args(std::env::args().skip(1))?;

    let xml = std::fs::read_to_string(&cfg.image_path)
        .with_context(|| format!("reading session image {}", cfg.image_path))?;
    let image = SessionImage::from_xml(&xml).context("parsing session image")?;
    let id = image.id.clone().unwrap_or_else(|| image.compute_id());

    tracing::info!(
        "serving session {id} ({} surfaces) at http://{}:{}/session/{id}",
        image.surfaces.len(),
        cfg.bind,
        cfg.port,
    );

    let served = ServedImage::new_xml(id, xml.into_bytes());
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    serve((cfg.bind.as_str(), cfg.port), served, shutdown).await
}

struct Config {
    image_path: String,
    bind: String,
    port: u16,
}

impl Config {
    fn from_args(args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut image_path = None;
        let mut bind = "0.0.0.0".to_string();
        let mut port: u16 = 7000;

        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bind" => bind = args.next().context("--bind needs a value")?,
                "--port" => {
                    port = args
                        .next()
                        .context("--port needs a value")?
                        .parse()
                        .context("--port must be a number")?
                }
                "-h" | "--help" => {
                    print_usage();
                    std::process::exit(0);
                }
                other if other.starts_with('-') => bail!("unknown flag: {other}"),
                other => {
                    if image_path.replace(other.to_string()).is_some() {
                        bail!("expected a single session image path");
                    }
                }
            }
        }

        Ok(Config {
            image_path: image_path.context("missing session image path")?,
            bind,
            port,
        })
    }
}

fn print_usage() {
    eprintln!("usage: simulboot-broker <session-image.xml> [--bind 0.0.0.0] [--port 7000]");
}
