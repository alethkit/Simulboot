//! Minimal HTTP server for serving a suspended session image.
//!
//! In v0 the session image *is* the broker: there is no broker UI and no
//! registry. When a compositor suspends, it parks the session image behind a
//! single endpoint:
//!
//! ```text
//! GET /session/{id}  ->  200 OK, the session image body
//! ```
//!
//! `{id}` may be given as the full `sha256:<hex>` id or just the bare hex. The
//! server stays up until it is dropped / its shutdown future fires — in the demo
//! flow that is "until all hosts have acknowledged Reconnect".
//!
//! The HTTP/1.1 handling here is hand-rolled and intentionally tiny: this serves
//! exactly one document to one or two trusted peers on a Tailscale network.
//!
//! The compositor's `session` module can reuse [`serve`] directly rather than
//! shelling out to the `simulboot-broker` binary.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use smol::io::{AsyncReadExt, AsyncWriteExt};
use smol::net::{AsyncToSocketAddrs, TcpListener, TcpStream};

/// A session image ready to be served.
#[derive(Debug, Clone)]
pub struct ServedImage {
    /// The full content id, e.g. `sha256:abcd...`.
    pub id: String,
    /// The serialised image body (text XML for v0).
    pub body: Vec<u8>,
    /// MIME type sent in the `Content-Type` header.
    ///
    /// The brief specifies Fast Infoset (`application/fastinfoset`) as the
    /// transmission form; v0 serves text XML, so this defaults to
    /// `application/xml`. Swap both the body and this field when FI lands.
    pub content_type: String,
}

impl ServedImage {
    /// Construct from a full id and an XML body, defaulting the content type to
    /// `application/xml; charset=utf-8`.
    pub fn new_xml(id: impl Into<String>, body: Vec<u8>) -> Self {
        ServedImage {
            id: id.into(),
            body,
            content_type: "application/xml; charset=utf-8".to_string(),
        }
    }

    /// The bare hex form of the id (the `sha256:` prefix stripped), used to match
    /// the `{session_id_hex}` form of the request path.
    fn bare_hex(&self) -> &str {
        self.id.strip_prefix("sha256:").unwrap_or(&self.id)
    }

    /// Does `requested` (the path segment after `/session/`) name this image?
    fn matches(&self, requested: &str) -> bool {
        requested == self.id || requested == self.bare_hex()
    }
}

/// Serve `image` over HTTP on `addr` until `shutdown` completes.
///
/// `addr` is anything that resolves to a [`std::net::SocketAddr`], e.g.
/// `"0.0.0.0:7000"`. Returns once the shutdown future fires; in-flight
/// connections are not forcibly drained (each is short-lived).
pub async fn serve<A, S>(addr: A, image: ServedImage, shutdown: S) -> anyhow::Result<()>
where
    A: AsyncToSocketAddrs,
    S: Future<Output = ()>,
{
    let listener = TcpListener::bind(addr).await.context("binding broker listener")?;
    let local = listener.local_addr().ok();
    if let Some(local) = local {
        tracing::info!(%local, id = %image.id, "broker serving session image");
    }
    let image = Arc::new(image);

    // Race shutdown against each accept; either branch yields an `Event`.
    enum Event {
        Shutdown,
        Accepted(std::io::Result<(TcpStream, SocketAddr)>),
    }

    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        let event = smol::future::or(
            async {
                shutdown.as_mut().await;
                Event::Shutdown
            },
            async { Event::Accepted(listener.accept().await) },
        )
        .await;

        match event {
            Event::Shutdown => {
                tracing::info!("broker shutting down");
                return Ok(());
            }
            Event::Accepted(accepted) => {
                let (stream, peer) = accepted.context("accepting connection")?;
                let image = Arc::clone(&image);
                smol::spawn(async move {
                    if let Err(e) = handle_connection(stream, &image).await {
                        tracing::warn!(%peer, error = %e, "broker connection error");
                    }
                })
                .detach();
            }
        }
    }
}

async fn handle_connection(mut stream: TcpStream, image: &ServedImage) -> anyhow::Result<()> {
    // Read until we have the request head (terminated by CRLFCRLF). We only need
    // the request line; bodies are irrelevant for GET.
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if find_header_end(&buf).is_some() || buf.len() > 64 * 1024 {
            break;
        }
    }

    let head = String::from_utf8_lossy(&buf);
    let request_line = head.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    let response = route(method, path, image);
    write_response(&mut stream, response, image).await
}

/// The outcome of routing a request, kept separate from I/O for testability.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// Serve the image body.
    Ok,
    NotFound,
    MethodNotAllowed,
}

fn route(method: &str, path: &str, image: &ServedImage) -> Route {
    if method != "GET" {
        return Route::MethodNotAllowed;
    }
    match path.strip_prefix("/session/") {
        Some(id) if image.matches(id.trim_end_matches('/')) => Route::Ok,
        _ => Route::NotFound,
    }
}

async fn write_response(
    stream: &mut TcpStream,
    route: Route,
    image: &ServedImage,
) -> anyhow::Result<()> {
    let head;
    let body: &[u8];
    match route {
        Route::Ok => {
            head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                image.content_type,
                image.body.len()
            );
            body = &image.body;
        }
        Route::NotFound => {
            body = b"session not found\n";
            head = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
        }
        Route::MethodNotAllowed => {
            body = b"method not allowed\n";
            head = format!(
                "HTTP/1.1 405 Method Not Allowed\r\nAllow: GET\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
        }
    }
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> ServedImage {
        ServedImage::new_xml("sha256:deadbeef", b"<session/>".to_vec())
    }

    #[test]
    fn routes_full_id() {
        assert_eq!(route("GET", "/session/sha256:deadbeef", &img()), Route::Ok);
    }

    #[test]
    fn routes_bare_hex() {
        assert_eq!(route("GET", "/session/deadbeef", &img()), Route::Ok);
    }

    #[test]
    fn unknown_id_is_404() {
        assert_eq!(route("GET", "/session/cafe", &img()), Route::NotFound);
    }

    #[test]
    fn post_is_405() {
        assert_eq!(route("POST", "/session/deadbeef", &img()), Route::MethodNotAllowed);
    }

    #[test]
    fn header_end_detection() {
        assert_eq!(find_header_end(b"GET / HTTP/1.1\r\n\r\n"), Some(14));
        assert_eq!(find_header_end(b"partial\r\n"), None);
    }
}
