//! Newline-delimited JSON over any async byte pair.
//!
//! One JSON object per line, no embedded newlines ([`Message::to_line`] is
//! compact, and serde_json never emits a raw newline inside a compact value).
//!
//! The read path keeps its own buffer rather than leaning on
//! [`tokio::io::Lines`], and that is deliberate: the peer drives `recv` inside
//! a `select!`, so the future is dropped every time an outbound message wins
//! the race. `Lines::next_line` is not cancellation safe and would lose a
//! partial line each time. Holding the partial bytes in a field that survives
//! the future makes cancellation free, which is what the [`Transport`] contract
//! requires.

use std::io;
use std::pin::Pin;

use async_trait::async_trait;
use lanok_core::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, stdin, stdout};

use crate::Transport;

/// Bytes read from the underlying stream per syscall.
const CHUNK: usize = 8 * 1024;

/// The longest single line accepted before the connection is failed.
///
/// Without a cap, a peer that never writes a newline is an unbounded
/// allocation. 64 MiB is far past any real payload and well short of a problem.
const MAX_LINE: usize = 64 * 1024 * 1024;

/// Newline-delimited JSON over an arbitrary reader and writer.
pub struct NdjsonTransport {
    reader: Pin<Box<dyn AsyncRead + Send>>,
    writer: Pin<Box<dyn AsyncWrite + Send>>,
    /// Bytes read but not yet consumed as a line. Survives a dropped `recv`
    /// future, which is what makes `recv` cancellation safe.
    buffer: Vec<u8>,
    eof: bool,
    skipped: u64,
}

impl std::fmt::Debug for NdjsonTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NdjsonTransport")
            .field("buffered_bytes", &self.buffer.len())
            .field("skipped_lines", &self.skipped)
            .field("eof", &self.eof)
            .finish_non_exhaustive()
    }
}

impl NdjsonTransport {
    pub fn new<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + 'static,
        W: AsyncWrite + Send + 'static,
    {
        NdjsonTransport {
            reader: Box::pin(reader),
            writer: Box::pin(writer),
            buffer: Vec::new(),
            eof: false,
            skipped: 0,
        }
    }

    /// How many unparseable lines have been skipped. Surfaced so a host can log
    /// "this server is writing non-protocol output to stdout", which is the
    /// single most common authoring mistake and otherwise presents as silence.
    pub fn skipped_lines(&self) -> u64 {
        self.skipped
    }

    /// Take the next complete line out of the buffer, if there is one.
    fn take_line(&mut self) -> Option<String> {
        let newline = self.buffer.iter().position(|&b| b == b'\n')?;
        let mut line: Vec<u8> = self.buffer.drain(..=newline).collect();
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Some(String::from_utf8_lossy(&line).into_owned())
    }
}

#[async_trait]
impl Transport for NdjsonTransport {
    async fn recv(&mut self) -> Option<io::Result<Message>> {
        loop {
            // Anything already buffered is consumed before touching the stream,
            // so a cancelled read never costs a message.
            while let Some(line) = self.take_line() {
                if line.trim().is_empty() {
                    continue;
                }
                match Message::from_line(&line) {
                    Ok(message) => return Some(Ok(message)),
                    Err(_) => {
                        self.skipped += 1;
                        continue;
                    }
                }
            }

            if self.eof {
                // A final line without a trailing newline is still a message.
                let trailing = std::mem::take(&mut self.buffer);
                let line = String::from_utf8_lossy(&trailing).trim().to_string();
                if line.is_empty() {
                    return None;
                }
                return match Message::from_line(&line) {
                    Ok(message) => Some(Ok(message)),
                    Err(_) => {
                        self.skipped += 1;
                        None
                    }
                };
            }

            if self.buffer.len() > MAX_LINE {
                return Some(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("peer sent more than {MAX_LINE} bytes without a newline"),
                )));
            }

            let mut chunk = [0u8; CHUNK];
            // `AsyncReadExt::read` is cancellation safe: dropped before it
            // resolves, no bytes were taken from the stream.
            match self.reader.read(&mut chunk).await {
                Ok(0) => self.eof = true,
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
                Err(e) => return Some(Err(e)),
            }
        }
    }

    async fn send(&mut self, message: Message) -> io::Result<()> {
        let mut line = message.to_line();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        // Flush per message. A protocol peer is interactive by nature, so a
        // buffered response that only arrives at process exit is a hang, not a
        // latency detail.
        self.writer.flush().await
    }

    async fn close(&mut self) -> io::Result<()> {
        self.writer.shutdown().await
    }
}

/// This process's own stdin and stdout.
///
/// The server side of a stdio protocol. Keep stdout clean: only protocol JSON
/// belongs there, and logging belongs on stderr.
#[derive(Debug)]
pub struct StdioTransport(NdjsonTransport);

impl StdioTransport {
    pub fn new() -> Self {
        StdioTransport(NdjsonTransport::new(stdin(), stdout()))
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn recv(&mut self) -> Option<io::Result<Message>> {
        self.0.recv().await
    }
    async fn send(&mut self, message: Message) -> io::Result<()> {
        self.0.send(message).await
    }
    async fn close(&mut self) -> io::Result<()> {
        self.0.close().await
    }
}

/// An in-memory transport pair, for driving both sides in one test.
///
/// The two halves are crossed: what one sends, the other receives.
pub fn duplex() -> (NdjsonTransport, NdjsonTransport) {
    const BUFFER: usize = 64 * 1024;
    let (a_read, b_write) = tokio::io::duplex(BUFFER);
    let (b_read, a_write) = tokio::io::duplex(BUFFER);
    (
        NdjsonTransport::new(a_read, a_write),
        NdjsonTransport::new(b_read, b_write),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[tokio::test]
    async fn carries_messages_both_ways() {
        let (mut a, mut b) = duplex();

        a.send(Message::request(1u64, "echo", json!({ "text": "hi" })))
            .await
            .unwrap();
        let received = b.recv().await.unwrap().unwrap();
        assert_eq!(received.method(), Some("echo"));

        // The reverse direction is the same transport, not a second one.
        b.send(Message::request(1u64, "ui/ask", json!({})))
            .await
            .unwrap();
        let received = a.recv().await.unwrap().unwrap();
        assert_eq!(received.method(), Some("ui/ask"));
    }

    #[tokio::test]
    async fn skips_junk_lines_without_killing_the_stream() {
        let input = concat!(
            "Listening on stdio...\n",
            "\n",
            "{ not json\n",
            "{\"id\":1,\"method\":\"survived\"}\n",
        );
        let mut transport = NdjsonTransport::new(input.as_bytes(), Vec::new());

        let message = transport.recv().await.unwrap().unwrap();
        assert_eq!(message.method(), Some("survived"));
        assert_eq!(transport.skipped_lines(), 2);
    }

    #[tokio::test]
    async fn recv_is_cancellation_safe() {
        // The peer drives recv inside a select!, so the future is dropped every
        // time an outbound message wins the race. A partial line must survive.
        let (client, mut server) = tokio::io::duplex(64 * 1024);
        let mut transport = NdjsonTransport::new(client, Vec::new());

        let half = br#"{"id":1,"method":"sur"#;
        server.write_all(half).await.unwrap();

        // Cancel the read repeatedly while the line is incomplete.
        for _ in 0..5 {
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(20), transport.recv())
                    .await
                    .is_err(),
                "an incomplete line must not resolve"
            );
        }

        server.write_all(b"vived\"}\n").await.unwrap();
        let message = transport.recv().await.unwrap().unwrap();
        assert_eq!(message.method(), Some("survived"));
    }

    #[tokio::test]
    async fn a_final_line_without_a_newline_is_still_a_message() {
        let mut transport = NdjsonTransport::new(&br#"{"id":1,"method":"last"}"#[..], Vec::new());
        let message = transport.recv().await.unwrap().unwrap();
        assert_eq!(message.method(), Some("last"));
        assert!(transport.recv().await.is_none());
    }

    #[tokio::test]
    async fn carriage_returns_are_tolerated() {
        let mut transport =
            NdjsonTransport::new(&b"{\"id\":1,\"method\":\"crlf\"}\r\n"[..], Vec::new());
        let message = transport.recv().await.unwrap().unwrap();
        assert_eq!(message.method(), Some("crlf"));
    }

    #[tokio::test]
    async fn eof_is_a_clean_close() {
        let mut transport = NdjsonTransport::new(&b""[..], Vec::new());
        assert!(transport.recv().await.is_none());
    }

    #[tokio::test]
    async fn a_dropped_peer_ends_the_stream() {
        let (mut a, b) = duplex();
        drop(b);
        assert!(a.recv().await.is_none());
    }

    #[tokio::test]
    async fn each_message_is_one_flushed_line() {
        let (mut a, mut b) = duplex();
        for n in 0..3u64 {
            a.send(Message::notification("tick", json!({ "n": n })))
                .await
                .unwrap();
        }
        for n in 0..3u64 {
            let message = b.recv().await.unwrap().unwrap();
            assert_eq!(message.to_value()["params"]["n"], n);
        }
    }
}
