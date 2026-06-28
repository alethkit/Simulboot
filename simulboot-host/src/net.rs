//! QUIC server transport for a host.
//!
//! The host is the QUIC *server*: it listens, the compositor dials in. Per the
//! brief, TLS uses a self-signed cert generated with `rcgen` (the compositor
//! skips verification because Tailscale has already authenticated the peer).
//!
//! Callers must install a rustls crypto provider once at process start, e.g.
//! `rustls::crypto::ring::default_provider().install_default()`.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use quinn::crypto::rustls::QuicServerConfig;
use quinn::{Endpoint, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use simulboot_common::ALPN;

/// Build a QUIC server endpoint bound to `bind`, presenting a freshly generated
/// self-signed certificate and advertising the simulboot ALPN.
///
/// In v0 the certificate is ephemeral (regenerated each run); persisting it to
/// disk on first run is a week-8 polish item and does not affect correctness
/// because the compositor does not verify it.
pub fn server_endpoint(bind: SocketAddr) -> Result<Endpoint> {
    let cert = rcgen::generate_simple_self_signed(vec!["simulboot-host".to_string()])
        .context("generating self-signed certificate")?;
    let cert_der: CertificateDer<'static> = cert.cert.der().clone();
    let key = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());

    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key.into())
        .context("installing self-signed certificate")?;
    tls.alpn_protocols = vec![ALPN.to_vec()];

    let quic = QuicServerConfig::try_from(tls).context("building QUIC server config")?;
    let endpoint = Endpoint::server(ServerConfig::with_crypto(Arc::new(quic)), bind)
        .with_context(|| format!("binding QUIC endpoint on {bind}"))?;
    Ok(endpoint)
}
