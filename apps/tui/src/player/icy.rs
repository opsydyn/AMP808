use std::io::{self, Read};

/// Wraps an inner Read, transparently stripping interleaved ICY metadata.
/// Fires on_meta callback with parsed StreamTitle.
///
/// ICY metadata protocol:
/// - Every `meta_int` bytes of audio data, a metadata block appears
/// - Metadata block: 1-byte length prefix (multiply by 16), then that many bytes of null-padded text
/// - Text format: `StreamTitle='Artist - Song';StreamUrl='...';`
pub struct IcyReader<R: Read> {
    inner: R,
    meta_int: usize,
    remaining: usize,
    on_meta: Box<dyn Fn(String) + Send + Sync>,
}

impl<R: Read> IcyReader<R> {
    pub fn new(
        inner: R,
        meta_int: usize,
        on_meta: impl Fn(String) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner,
            meta_int,
            remaining: meta_int,
            on_meta: Box::new(on_meta),
        }
    }

    /// Read and process a metadata block.
    fn consume_meta(&mut self) -> io::Result<()> {
        // Read 1-byte length prefix
        let mut len_buf = [0u8; 1];
        read_exact(&mut self.inner, &mut len_buf)?;

        let meta_len = len_buf[0] as usize * 16;
        if meta_len == 0 {
            return Ok(());
        }

        let mut buf = vec![0u8; meta_len];
        read_exact(&mut self.inner, &mut buf)?;

        // Trim null padding
        let meta = String::from_utf8_lossy(&buf)
            .trim_end_matches('\0')
            .to_string();

        if let Some(title) = parse_stream_title(&meta) {
            (self.on_meta)(title);
        }

        Ok(())
    }
}

impl<R: Read> Read for IcyReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            self.consume_meta()?;
            self.remaining = self.meta_int;
        }

        // Clamp read to not cross into metadata block
        let max = buf.len().min(self.remaining);
        let n = self.inner.read(&mut buf[..max])?;
        self.remaining -= n;
        Ok(n)
    }
}

/// Extract StreamTitle from ICY metadata string.
/// Format: `StreamTitle='Artist - Song';StreamUrl='...';`
/// Tolerates missing trailing semicolon (matches Go implementation).
fn parse_stream_title(meta: &str) -> Option<String> {
    let prefix = "StreamTitle='";
    let i = meta.find(prefix)?;
    let rest = &meta[i + prefix.len()..];

    let end = if let Some(j) = rest.find("';") {
        j
    } else {
        // Tolerate missing semicolon — find last single quote
        rest.rfind('\'')?
    };

    let title = &rest[..end];
    if title.is_empty() {
        return None;
    }
    Some(title.to_string())
}

/// Read exactly `buf.len()` bytes, returning an error on short reads.
fn read_exact<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<()> {
    let mut pos = 0;
    while pos < buf.len() {
        match reader.read(&mut buf[pos..])? {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF in ICY metadata",
                ));
            }
            n => pos += n,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Build a synthetic ICY stream: audio bytes interleaved with metadata blocks.
    fn build_icy_stream(audio: &[u8], meta_int: usize, metadata: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut audio_pos = 0;

        while audio_pos < audio.len() {
            // Write up to meta_int bytes of audio
            let chunk = meta_int.min(audio.len() - audio_pos);
            out.extend_from_slice(&audio[audio_pos..audio_pos + chunk]);
            audio_pos += chunk;

            if audio_pos <= audio.len() {
                // Write metadata block
                let meta_bytes = metadata.as_bytes();
                let meta_len = meta_bytes.len().div_ceil(16);
                let padded_len = meta_len * 16;

                out.push(meta_len as u8);
                out.extend_from_slice(meta_bytes);
                // Null-pad to multiple of 16
                out.extend(std::iter::repeat_n(0, padded_len - meta_bytes.len()));
            }
        }

        out
    }

    #[test]
    fn test_icy_reader_strips_metadata() {
        let audio = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let meta_int = 4;
        let metadata = "StreamTitle='Test Artist - Test Song';";
        let stream = build_icy_stream(&audio, meta_int, metadata);

        let titles: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let titles_cb = Arc::clone(&titles);

        let mut reader = IcyReader::new(&stream[..], meta_int, move |title| {
            titles_cb.lock().unwrap().push(title);
        });

        // Read all audio through the IcyReader
        let mut output = Vec::new();
        let mut buf = [0u8; 32];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }

        // Audio should pass through with metadata stripped
        assert_eq!(output, audio);

        // Title callback should have fired
        let titles = titles.lock().unwrap();
        assert!(!titles.is_empty());
        assert_eq!(titles[0], "Test Artist - Test Song");
    }

    #[test]
    fn test_icy_reader_empty_metadata() {
        // Stream with zero-length metadata blocks (no metadata)
        let audio = vec![1u8, 2, 3, 4];
        let meta_int = 4;

        let mut stream = Vec::new();
        stream.extend_from_slice(&audio);
        stream.push(0); // zero-length metadata block

        let mut reader = IcyReader::new(&stream[..], meta_int, |_| {});

        let mut output = Vec::new();
        let mut buf = [0u8; 32];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }

        assert_eq!(output, audio);
    }

    #[test]
    fn test_parse_stream_title_standard() {
        let meta = "StreamTitle='Foo - Bar';StreamUrl='http://example.com';";
        assert_eq!(parse_stream_title(meta), Some("Foo - Bar".to_string()));
    }

    #[test]
    fn test_parse_stream_title_no_semicolon() {
        // Tolerate missing semicolon
        let meta = "StreamTitle='Foo - Bar'";
        assert_eq!(parse_stream_title(meta), Some("Foo - Bar".to_string()));
    }

    #[test]
    fn test_parse_stream_title_empty() {
        let meta = "StreamTitle='';";
        assert_eq!(parse_stream_title(meta), None);
    }

    #[test]
    fn test_parse_stream_title_missing() {
        let meta = "SomeOtherKey='value';";
        assert_eq!(parse_stream_title(meta), None);
    }

    #[test]
    fn test_parse_stream_title_with_null_padding() {
        let meta = "StreamTitle='Live Radio'\0\0\0\0\0";
        assert_eq!(parse_stream_title(meta), Some("Live Radio".to_string()));
    }
}
