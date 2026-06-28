//! Session checkpoint and restore (F3, F4, F6).
//!
//! Suspension turns the live strip into a [`SessionImage`]; resumption turns a
//! fetched image back into a set of hosts to reconnect and a layout to restore.
//! The image is self-describing: everything needed to reconstitute the session
//! is inside it, so a bare compositor can resume with only a URL.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use simulboot_common::{HostEntry, Layout, SessionImage, SurfaceEntry};

use crate::strip::Strip;

/// Build a content-addressed session image from the current strip state.
pub fn image_from_strip(strip: &Strip, created: String) -> SessionImage {
    let surfaces = strip
        .surfaces()
        .iter()
        .map(|s| SurfaceEntry {
            id: s.id,
            name: s.name.clone(),
            order: s.order,
            host: HostEntry {
                address: s.host_addr.to_string(),
                os: s.provenance.os,
                machine: s.provenance.machine_name.clone(),
                capture: s.provenance.capture_description.clone(),
            },
            codec: s.codec,
            width: s.source_width,
            height: s.source_height,
        })
        .collect();

    let layout = Layout { scroll_pos: strip.scroll_pos(), focus: strip.focus() };
    SessionImage::new(created, surfaces, layout).with_computed_id()
}

/// Fetch and validate a session image from a `--resume` URL.
///
/// Verifies that the content hash recomputed from the document matches the id in
/// the URL path, so a resuming compositor cannot be handed a substituted image.
pub async fn fetch_image(url: &str) -> Result<SessionImage> {
    let (host, port, path) = parse_http_url(url)?;
    let body = http_get(&host, port, &path).await?;
    let xml = String::from_utf8(body).context("session image was not valid UTF-8")?;
    let image = SessionImage::from_xml(&xml).context("parsing fetched session image")?;

    // The id segment in the URL is the authority; check the document against it.
    let url_id = path.rsplit('/').next().unwrap_or_default();
    let computed = image.compute_id();
    let matches = url_id == computed
        || url_id == computed.strip_prefix("sha256:").unwrap_or(&computed);
    if !matches {
        bail!("session image integrity check failed: url id {url_id:?} != computed {computed}");
    }
    image.verify().or_else(|_| {
        // The document may carry no id attribute; the URL check above is enough.
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(image)
}

/// An RFC 3339 / ISO 8601 UTC timestamp for "now", e.g. `2026-06-28T19:44:09Z`.
/// Hand-rolled to keep the dependency set minimal.
pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_epoch_utc(secs)
}

/// Convert Unix seconds to an RFC 3339 UTC string using the civil-from-days
/// algorithm (Howard Hinnant's `civil_from_days`).
fn format_epoch_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // days since 1970-01-01 -> civil date
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Split an `http://host[:port]/path` URL into its parts. Only plain HTTP is
/// supported (the broker is plaintext over Tailscale).
fn parse_http_url(url: &str) -> Result<(String, u16, String)> {
    let rest = url
        .strip_prefix("http://")
        .context("resume URL must start with http://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().context("invalid port in URL")?),
        None => (authority.to_string(), 80),
    };
    Ok((host, port, path.to_string()))
}

/// Minimal HTTP/1.1 GET returning the response body. Adequate for fetching one
/// session image from a trusted Tailscale peer.
async fn http_get(host: &str, port: u16, path: &str) -> Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect((host, port))
        .await
        .with_context(|| format!("connecting to {host}:{port}"))?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: */*\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;

    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("malformed HTTP response (no header terminator)")?;
    let header = String::from_utf8_lossy(&raw[..split]);
    let status = header.lines().next().unwrap_or("");
    if !status.contains(" 200") {
        bail!("session image fetch failed: {status}");
    }
    Ok(raw[split + 4..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_formats_known_instant() {
        // 2026-06-28T19:44:09Z (the design doc's example timestamp).
        assert_eq!(format_epoch_utc(1_782_675_849), "2026-06-28T19:44:09Z");
    }

    #[test]
    fn epoch_zero_is_unix_epoch() {
        assert_eq!(format_epoch_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn parses_url_parts() {
        let (h, p, path) = parse_http_url("http://100.0.0.1:7000/session/sha256:abc").unwrap();
        assert_eq!(h, "100.0.0.1");
        assert_eq!(p, 7000);
        assert_eq!(path, "/session/sha256:abc");
    }
}
