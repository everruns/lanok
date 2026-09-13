//! A transport over a spawned child process.
//!
//! The host side of a stdio protocol. Two details matter more than they look:
//!
//! * **stderr is drained.** A server that logs to stderr fills the pipe buffer
//!   and blocks forever if nobody reads it, which presents as "my extension
//!   hangs after about 8 KiB of logging" and is miserable to diagnose. The
//!   drain runs in its own task from the moment the child starts.
//! * **closing is a sequence, not a kill.** [`Transport::close`] shuts stdin
//!   (the polite exit signal), gives the child a grace period to exit on its
//!   own, kills only if it overstays, and then waits for the stderr drain to
//!   finish. That last step is what puts a crashing server's final log lines in
//!   front of whoever is debugging it: killing first discards whatever was
//!   still sitting in the pipe.

use std::io;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use lanok_core::Message;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

use crate::{NdjsonTransport, Transport};

/// How long a child gets to exit on its own after its stdin closes, before it
/// is killed. Long enough for a serve loop to finish a flush, short enough that
/// a wedged server does not hold up a host's shutdown.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// How long `close` waits for queued stderr to reach the sink once the child is
/// gone. Bounded so a pathological writer cannot hang the caller.
const DRAIN_GRACE: Duration = Duration::from_secs(1);

/// Where a child's stderr lines go. Called once per line, off the hot path.
pub type StderrSink = Arc<dyn Fn(&str) + Send + Sync>;

/// A child process speaking ndjson over its stdin and stdout.
pub struct ChildTransport {
    inner: NdjsonTransport,
    child: Option<Child>,
    stderr_drain: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ChildTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildTransport")
            .field("pid", &self.child.as_ref().and_then(Child::id))
            .finish_non_exhaustive()
    }
}

impl ChildTransport {
    /// Spawn `command`, discarding its stderr.
    pub fn spawn(command: Command) -> io::Result<Self> {
        Self::spawn_with_stderr(command, None)
    }

    /// Spawn `command`, sending each stderr line to `sink`.
    pub fn spawn_logging(command: Command, sink: StderrSink) -> io::Result<Self> {
        Self::spawn_with_stderr(command, Some(sink))
    }

    fn spawn_with_stderr(mut command: Command, sink: Option<StderrSink>) -> io::Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Without this, killing the handle leaves the child running when
            // the parent exits abnormally.
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        let missing = |what: &str| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("child process has no {what}"),
            )
        };
        let stdout = child.stdout.take().ok_or_else(|| missing("stdout"))?;
        let stdin = child.stdin.take().ok_or_else(|| missing("stdin"))?;
        let stderr = child.stderr.take().ok_or_else(|| missing("stderr"))?;

        // Drain stderr unconditionally, sink or not: an unread pipe is what
        // deadlocks a chatty server, and discarding still requires reading.
        let stderr_drain = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(sink) = &sink {
                    sink(&line);
                }
            }
        });

        Ok(ChildTransport {
            inner: NdjsonTransport::new(stdout, stdin),
            child: Some(child),
            stderr_drain: Some(stderr_drain),
        })
    }

    /// How many unparseable lines the child wrote to stdout. Non-zero means the
    /// server is printing something other than protocol JSON.
    pub fn skipped_lines(&self) -> u64 {
        self.inner.skipped_lines()
    }

    /// The child's process id, while it is running.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }
}

#[async_trait]
impl Transport for ChildTransport {
    async fn recv(&mut self) -> Option<io::Result<Message>> {
        self.inner.recv().await
    }

    async fn send(&mut self, message: Message) -> io::Result<()> {
        self.inner.send(message).await
    }

    async fn close(&mut self) -> io::Result<()> {
        // Closing stdin is the polite shutdown signal: a serve loop sees EOF
        // and returns.
        let _ = self.inner.close().await;

        if let Some(mut child) = self.child.take() {
            // Give it a chance to exit on its own before reaching for the kill,
            // so an orderly shutdown path actually gets to run.
            if timeout(EXIT_GRACE, child.wait()).await.is_err() {
                let _ = child.kill().await;
            }
        }

        // The child is gone, so stderr is at EOF and the drain finishes on its
        // own. Waiting for it is what delivers the last lines a dying server
        // wrote, which are the ones worth having.
        if let Some(drain) = self.stderr_drain.take() {
            if timeout(DRAIN_GRACE, drain).await.is_err() {
                // A writer still going after the grace period is not worth
                // blocking a host's shutdown on.
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;

    /// A server built from shell: reads one line, answers it, exits.
    fn echoing_server() -> Command {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(r#"read -r line; echo '{"jsonrpc":"2.0","id":1,"result":{"ok":true}}'"#);
        command
    }

    #[tokio::test]
    async fn round_trips_through_a_real_process() {
        let mut transport = ChildTransport::spawn(echoing_server()).unwrap();
        assert!(transport.pid().is_some());

        transport
            .send(Message::request(1u64, "ping", json!({})))
            .await
            .unwrap();

        let response = transport.recv().await.unwrap().unwrap();
        match response {
            Message::Response { payload: Ok(v), .. } => assert_eq!(v["ok"], true),
            other => panic!("expected a result, got {other:?}"),
        }
        transport.close().await.unwrap();
    }

    #[tokio::test]
    async fn stderr_reaches_the_sink() {
        let collected = Arc::new(Mutex::new(Vec::new()));
        let sink_target = collected.clone();
        let sink: StderrSink = Arc::new(move |line: &str| {
            sink_target.lock().unwrap().push(line.to_string());
        });

        let mut command = Command::new("sh");
        command.arg("-c").arg("echo 'starting up' >&2; read -r _");
        let mut transport = ChildTransport::spawn_logging(command, sink).unwrap();

        // close() closes stdin (so the child's `read` returns and it exits),
        // waits for the exit, then waits for the drain. The lines must be in
        // hand by the time it returns: a host debugging a server that died on
        // startup has nothing else to go on.
        transport.close().await.unwrap();
        assert_eq!(collected.lock().unwrap().as_slice(), ["starting up"]);
    }

    #[tokio::test]
    async fn a_chatty_server_does_not_deadlock() {
        // 4 MiB of stderr: far past any pipe buffer. If stderr were not drained
        // this test would hang rather than fail, which is exactly the bug.
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(r#"yes 'noise noise noise noise noise noise' | head -n 70000 >&2; echo '{"jsonrpc":"2.0","id":1,"result":"done"}'"#);

        let mut transport = ChildTransport::spawn(command).unwrap();
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), transport.recv())
            .await
            .expect("a drained stderr must not block stdout")
            .unwrap()
            .unwrap();

        match response {
            Message::Response { payload: Ok(v), .. } => assert_eq!(v, "done"),
            other => panic!("expected a result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_server_that_exits_immediately_reads_as_a_clean_close() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("exit 0");
        let mut transport = ChildTransport::spawn(command).unwrap();
        assert!(transport.recv().await.is_none());
    }
}
