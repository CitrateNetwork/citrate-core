//! Downloads. The trait keeps the installer independent of the transport; the production
//! transport is [`HttpsFetcher`]. Nothing fetched is trusted until the installer has checked
//! its size, hash and signature.
use std::io::{Read, Write};
use std::time::Duration;

use crate::error::ComponentError;
use crate::manifest::is_https_url;

pub trait Fetcher {
    /// Streams `url` into `sink`, failing with [`ComponentError::ArtifactTooLarge`] as soon as
    /// more than `max_bytes` arrive. Returns the number of bytes written.
    fn fetch(&self, url: &str, max_bytes: u64, sink: &mut dyn Write)
        -> Result<u64, ComponentError>;
}

/// Copies at most `max_bytes` from `reader` to `sink`; one byte more is an error.
pub fn copy_capped(
    reader: impl Read,
    sink: &mut dyn Write,
    max_bytes: u64,
) -> Result<u64, ComponentError> {
    let mut limited = reader.take(max_bytes.saturating_add(1));
    let n = std::io::copy(&mut limited, sink).map_err(|e| ComponentError::Fetch(e.to_string()))?;
    if n > max_bytes {
        return Err(ComponentError::ArtifactTooLarge { limit: max_bytes });
    }
    Ok(n)
}

/// HTTPS only (redirects included), bounded connect and header waits, a bounded total time for
/// the whole download, and a size-capped body.
pub struct HttpsFetcher {
    pub connect_timeout: Duration,
    pub response_timeout: Duration,
    /// The most the body may take to arrive, and the most the whole call may take. A stalled or
    /// trickling server ends the download instead of holding it open forever.
    pub body_timeout: Duration,
}

/// The default bound on one download (the largest component is a few hundred MB).
pub const DEFAULT_BODY_TIMEOUT: Duration = Duration::from_secs(30 * 60);

impl Default for HttpsFetcher {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(30),
            response_timeout: Duration::from_secs(90),
            body_timeout: DEFAULT_BODY_TIMEOUT,
        }
    }
}

impl Fetcher for HttpsFetcher {
    fn fetch(
        &self,
        url: &str,
        max_bytes: u64,
        sink: &mut dyn Write,
    ) -> Result<u64, ComponentError> {
        if !is_https_url(url) {
            return Err(ComponentError::Fetch(format!(
                "refusing a non-https url: {url}"
            )));
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(5)
            .timeout_connect(Some(self.connect_timeout))
            .timeout_recv_response(Some(self.response_timeout))
            .timeout_recv_body(Some(self.body_timeout))
            .timeout_global(Some(
                self.connect_timeout
                    .saturating_add(self.response_timeout)
                    .saturating_add(self.body_timeout),
            ))
            .build()
            .into();
        let resp = agent
            .get(url)
            .call()
            .map_err(|e| ComponentError::Fetch(e.to_string()))?;
        if resp.status().as_u16() != 200 {
            return Err(ComponentError::Fetch(format!(
                "HTTP {}",
                resp.status().as_u16()
            )));
        }
        let reader = resp
            .into_body()
            .into_with_config()
            .limit(max_bytes.saturating_add(1))
            .reader();
        copy_capped(reader, sink, max_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_capped_allows_exactly_the_limit() {
        let mut out = Vec::new();
        assert_eq!(copy_capped(&b"12345"[..], &mut out, 5), Ok(5));
        let mut out = Vec::new();
        assert_eq!(
            copy_capped(&b"123456"[..], &mut out, 5),
            Err(ComponentError::ArtifactTooLarge { limit: 5 })
        );
    }

    #[test]
    fn every_download_has_a_bounded_total_time() {
        let f = HttpsFetcher::default();
        assert_eq!(f.body_timeout, DEFAULT_BODY_TIMEOUT);
        assert!(f.body_timeout > Duration::ZERO && f.body_timeout <= Duration::from_secs(3600));
        let src = include_str!("fetch.rs");
        let call = src.find("fn fetch(").map(|i| &src[i..]).unwrap_or_default();
        assert!(call.contains(".timeout_recv_body(") && call.contains(".timeout_global("));
    }

    #[test]
    fn the_https_fetcher_refuses_plain_http_before_any_network() {
        let mut out = Vec::new();
        let err = HttpsFetcher::default()
            .fetch("http://example.invalid/x", 10, &mut out)
            .unwrap_err();
        assert!(matches!(err, ComponentError::Fetch(m) if m.contains("non-https")));
    }
}
