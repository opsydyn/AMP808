use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use symphonia::core::io::MediaSource;

use super::icy::IcyReader;

/// HTTP client with 30s connect timeout (matches Go's ResponseHeaderTimeout).
fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

/// MediaSource backed by an HTTP response body.
/// Non-seekable: Seek returns error, is_seekable() returns false.
/// Optionally wraps the body in IcyReader for ICY metadata extraction.
pub struct HttpMediaSource {
    reader: Box<dyn Read + Send + Sync>,
}

impl HttpMediaSource {
    /// Open an HTTP URL. Sends `Icy-MetaData: 1` header.
    /// If `stream_title` is provided and the server returns `Icy-Metaint`,
    /// wraps the body in IcyReader for transparent metadata extraction.
    pub fn open(url: &str, stream_title: Option<Arc<RwLock<String>>>) -> anyhow::Result<Self> {
        let client = http_client();
        let resp = client
            .get(url)
            .header("Icy-MetaData", "1")
            .send()
            .map_err(|e| anyhow::anyhow!("http get: {e}"))?;

        if !resp.status().is_success() {
            anyhow::bail!("http status {}", resp.status());
        }

        let meta_int: Option<usize> = resp
            .headers()
            .get("icy-metaint")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());

        let reader: Box<dyn Read + Send + Sync> =
            if let (Some(mi), Some(title_ref)) = (meta_int, stream_title) {
                Box::new(IcyReader::new(resp, mi, move |title| {
                    *title_ref.write().unwrap() = title;
                }))
            } else {
                Box::new(resp)
            };

        Ok(Self { reader })
    }
}

impl Read for HttpMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl Seek for HttpMediaSource {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "HTTP streams are not seekable",
        ))
    }
}

impl MediaSource for HttpMediaSource {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_media_source_not_seekable() {
        // Verify the trait contract without making real HTTP requests
        let source = HttpMediaSource {
            reader: Box::new(io::empty()),
        };
        assert!(!source.is_seekable());
        assert_eq!(source.byte_len(), None);
    }

    #[test]
    fn test_http_media_source_seek_returns_error() {
        let mut source = HttpMediaSource {
            reader: Box::new(io::empty()),
        };
        let result = source.seek(SeekFrom::Start(0));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
    }
}
