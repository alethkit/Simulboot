//! QUIC client transport for the compositor.
//!
//! The compositor dials each host. It skips certificate verification entirely —
//! Tailscale has already mutually authenticated and encrypted the path, so the
//! self-signed host cert is just there to satisfy TLS (brief, macOS Quinn note).
//!
//! Callers must install a rustls crypto provider once at process start.

use std::sync::Arc;

use anyhow::{Context, Result};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Endpoint, EndpointConfig, SmolRuntime};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use simulboot_common::ALPN;

/// Build a QUIC client endpoint bound to an ephemeral local port, configured to
/// accept any server certificate and advertise the simulboot ALPN.
pub fn client_endpoint() -> Result<Endpoint> {
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];

    let quic = QuicClientConfig::try_from(tls).context("building QUIC client config")?;

    // Runtime-agnostic construction on the smol runtime (no server config: this
    // endpoint only dials out). An ephemeral local UDP port.
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").context("binding client UDP socket")?;
    let mut endpoint = Endpoint::new(EndpointConfig::default(), None, socket, Arc::new(SmolRuntime))
        .context("creating client endpoint")?;
    endpoint.set_default_client_config(ClientConfig::new(Arc::new(quic)));
    Ok(endpoint)
}

/// A verifier that accepts every certificate. Safe here only because Tailscale
/// is the actual authenticator; never use this on the open internet.
#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
