//! Newline-delimited JSON over any async byte pair.
//!
//! One JSON object per line, no embedded newlines (serde_json never emits one
//! inside a compact value, and [`Message::to_line`] is compact). A line that
//! does not parse is counted and skipped rather than failing the stream: a peer
//! that writes one bad line, or a stray banner on stdout, should not take down
//! a connection that is otherwise fine.

use std::io;
use std::pin::Pin;

use async_trait::async_trait;
use lanok_core::Message;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, Lines, stdin, stdout,
};

use crate::Transport;

/// Newline-delimited JSON over an arbitrary reader and writer.
pub struct NdjsonTransport {
    lines: Lines<BufReader<Pin<Box<dyn AsyncRead + Send>>>>,
    writer: Pin<Box<dyn AsyncWrite + Send>>,
    skipped: u64,
}

impl std::fmt::Debug for NdjsonTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NdjsonTransport")
            .field("skipped_lines", &self.skipped)
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
            lines: BufReader::new(Box::pin(reader) as Pin<Box<dyn AsyncRead + Send>>).lines(),
            writer: Box::pin(writer),
            skipped: 0,
        }
    }

    /// How many unparseable lines have been skipped. Surfaced so a host can log
    /// "this server is writing non-protocol output to stdout", which is the
    /// single most common authoring mistake.
    pub fn skipped_lines(&self) -> u64 {
        self.skipped
    }
}

#[async_trait]
impl Transport for NdjsonTransport {
    async fn recv(&mut self) -> Option<io::Result<Message>> {
        loop {
            match self.lines.next_line().await {
                Ok(None) => return None,
                Ok(Some(line)) => {
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
                Err(e) => return Some(Err(e)),
            }
        }
    }

    async fn send(&mut self, message: Message) -> io::Result<()> {
        let mut line = message.to_line();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        // Flush per message. A protocol peer is interactive by nature, so a
        // buffered response that arrives at process exit is a hang, not a
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
            let value = message.to_value();
            assert_eq!(value["params"]["n"], n);
        }
    }
}
