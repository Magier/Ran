use tokio::sync::mpsc;

/// Which process stream produced an execution-output fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub(crate) struct OutputFragment {
    pub(crate) stream: OutputStream,
    pub(crate) bytes: Vec<u8>,
}

/// Cheap, cloneable output callback passed through every command backend.
///
/// Backends write fragments immediately so their pipes are continuously
/// drained. The executor owns batching and publication, keeping timing and
/// ordering policy out of individual transports.
#[derive(Clone, Debug, Default)]
pub struct OutputSink {
    tx: Option<mpsc::UnboundedSender<OutputFragment>>,
}

impl OutputSink {
    pub(crate) fn channel() -> (Self, mpsc::UnboundedReceiver<OutputFragment>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx: Some(tx) }, rx)
    }

    /// A sink for compatibility paths and tests that do not observe progress.
    pub fn discard() -> Self {
        Self::default()
    }

    pub fn stdout(&self, bytes: impl Into<Vec<u8>>) {
        self.send(OutputStream::Stdout, bytes.into());
    }

    pub fn stderr(&self, bytes: impl Into<Vec<u8>>) {
        self.send(OutputStream::Stderr, bytes.into());
    }

    fn send(&self, stream: OutputStream, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(OutputFragment { stream, bytes });
        }
    }
}

/// Incrementally decodes process bytes without corrupting a UTF-8 code point
/// split across two reads. Invalid byte sequences use the same replacement
/// behavior as the final `String::from_utf8_lossy` conversion.
#[derive(Default)]
pub(crate) struct IncrementalTextDecoder {
    pending: Vec<u8>,
}

impl IncrementalTextDecoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        self.decode(false)
    }

    pub(crate) fn finish(&mut self) -> String {
        self.decode(true)
    }

    fn decode(&mut self, finish: bool) -> String {
        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    output.push_str(valid);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    if valid_up_to > 0 {
                        output.push_str(
                            std::str::from_utf8(&self.pending[..valid_up_to])
                                .expect("validated UTF-8 prefix"),
                        );
                        self.pending.drain(..valid_up_to);
                    }
                    match error.error_len() {
                        Some(length) => {
                            output.push('\u{fffd}');
                            self.pending.drain(..length);
                        }
                        None if finish => {
                            output.push_str(&String::from_utf8_lossy(&self.pending));
                            self.pending.clear();
                            break;
                        }
                        None => break,
                    }
                }
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::IncrementalTextDecoder;

    #[test]
    fn preserves_utf8_split_across_fragments() {
        let mut decoder = IncrementalTextDecoder::default();
        assert_eq!(decoder.push(&[0xe2, 0x82]), "");
        assert_eq!(decoder.push(&[0xac, b'!']), "€!");
        assert_eq!(decoder.finish(), "");
    }

    #[test]
    fn replaces_invalid_bytes() {
        let mut decoder = IncrementalTextDecoder::default();
        assert_eq!(decoder.push(&[b'a', 0xff, b'b']), "a�b");
    }
}
