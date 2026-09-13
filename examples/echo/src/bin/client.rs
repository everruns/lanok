//! Drives a real `echo-server` child process end to end.
//!
//! This is the protocol's integration test in executable form: spawn, hand
//! shake, call forward, answer a reverse request, and assert on what came back.
//! It fails loudly, so CI can just run it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use echo_protocol::*;
use lanok::{ChildTransport, Hello, Peer, RpcError};
use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let blocking = args.iter().any(|arg| arg == "--blocking");

    // `--server <cmd> [args...]` drives an arbitrary implementation, which is
    // how the Python and TypeScript servers are exercised against the same
    // client. Without it, the Rust server next to this binary.
    let mut command = match args.iter().position(|arg| arg == "--server") {
        Some(at) => {
            let rest = &args[at + 1..];
            let (program, program_args) = rest.split_first().ok_or("--server needs a command")?;
            let mut command = Command::new(program);
            command.args(program_args);
            command
        }
        None => {
            let mut command = Command::new(server_binary()?);
            if !blocking {
                command.arg("--async");
            }
            command
        }
    };
    let _ = &mut command;
    // Server logs go to stderr; only protocol JSON belongs on stdout.
    let transport = ChildTransport::spawn_logging(
        command,
        Arc::new(|line: &str| eprintln!("[server] {line}")),
    )?;

    let progress_seen = Arc::new(AtomicU32::new(0));
    let ours = Hello::new("echo-client", PROTOCOL_VERSION).capability(capability::UI_ASK);

    let peer = Peer::builder()
        .handler(InitiatorDispatch::new(Client {
            progress_seen: progress_seen.clone(),
        }))
        .serve_handshake(ours.clone())
        .request_timeout(std::time::Duration::from_secs(30))
        .connect(transport);

    let server = peer.handshake(&ours, NEGOTIATION).await?;
    println!(
        "connected to {} speaking {} ({} capabilities)",
        server.name,
        server.protocol_version,
        server.capabilities.len()
    );

    peer.ping().await?;
    println!("ping ok");

    // The blocking server has no reverse channel, so it honours `shout`
    // directly; the async one asks the client instead. Both must end up
    // shouting, by different routes.
    let result = peer
        .echo(EchoParams {
            text: "hello from lanok".into(),
            shout: true,
        })
        .await?;
    println!("echo -> {}", result.text);

    assert_eq!(result.text, "HELLO FROM LANOK");
    assert_eq!(
        progress_seen.load(Ordering::SeqCst),
        3,
        "the server streams three progress notifications while echo is open"
    );
    if !blocking {
        assert!(
            server.capabilities.supports(capability::UI_ASK),
            "a concurrent server advertises the reverse capability"
        );
    }

    println!(
        "ok: {} flavour, {} progress notifications, reverse channel {}",
        if blocking { "blocking" } else { "async" },
        progress_seen.load(Ordering::SeqCst),
        if blocking { "unused" } else { "exercised" }
    );
    Ok(())
}

/// The server binary sits next to this one, whatever profile built it.
fn server_binary() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let mut path = std::env::current_exe()?;
    path.pop();
    path.push(format!("echo-server{}", std::env::consts::EXE_SUFFIX));
    if !path.exists() {
        return Err(format!("{} has not been built", path.display()).into());
    }
    Ok(path)
}

struct Client {
    progress_seen: Arc<AtomicU32>,
}

#[lanok::async_trait]
impl InitiatorHandler for Client {
    /// The reverse request: the server is asking us.
    async fn ui_ask(&self, params: AskParams) -> Result<AskResult, RpcError> {
        println!("server asks: {}", params.question);
        Ok(AskResult {
            answer: "yes".into(),
        })
    }

    fn progress(&self, params: ProgressParams) {
        println!("progress {}/{}", params.step, params.of);
        self.progress_seen.fetch_add(1, Ordering::SeqCst);
    }
}
