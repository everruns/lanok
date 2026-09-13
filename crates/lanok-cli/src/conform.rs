//! Replaying conformance vectors against a server process.
//!
//! The vectors are the protocol's behaviour written down once, in JSON, and
//! replayed against an implementation in any language. That is what makes a
//! Python or TypeScript server a first-class citizen rather than something
//! whose correctness is a matter of opinion: the same file that checks the Rust
//! server checks theirs.
//!
//! Expectations are deliberately *partial*. A case asserts what matters (an
//! error code, a few result fields, the notifications that must arrive first)
//! and says nothing about the rest, so adding an optional field to a payload
//! does not invalidate the suite. That is the same forward-compatibility rule
//! the wire itself follows.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};

/// One committed conformance suite.
#[derive(Debug, Deserialize)]
pub struct Suite {
    pub protocol: String,
    pub version: String,
    pub cases: Vec<Case>,
}

/// One exchange: send this, expect that.
#[derive(Debug, Deserialize)]
pub struct Case {
    pub name: String,
    /// The message to write, verbatim.
    pub send: Value,
    #[serde(default)]
    pub expect: Expect,
}

#[derive(Debug, Default, Deserialize)]
pub struct Expect {
    /// The response must be an error with this code.
    #[serde(default)]
    pub error_code: Option<i64>,
    /// These keys must be present in the result, with these values. Other keys
    /// are ignored, so an implementation may return more than the suite knows.
    #[serde(default)]
    pub result_contains: Option<Value>,
    /// These notification methods must arrive, in this order, before the
    /// response. Other notifications in between are ignored.
    #[serde(default)]
    pub notifications: Vec<String>,
    /// No response is expected at all (the sent message was a notification).
    #[serde(default)]
    pub no_response: bool,
}

/// A server under test.
struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    fn spawn(command: &[String]) -> Result<Self> {
        let (program, args) = command
            .split_first()
            .ok_or_else(|| anyhow!("no server command given after `--`"))?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited on purpose: a server's own logging is the most useful
            // thing on screen when a vector fails.
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("could not start `{program}`"))?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout was piped"));
        Ok(Server {
            child,
            stdin,
            stdout,
        })
    }

    fn send(&mut self, message: &Value) -> Result<()> {
        writeln!(self.stdin, "{message}")?;
        self.stdin.flush()?;
        Ok(())
    }

    /// The next message the server writes, skipping anything unparseable.
    fn next_message(&mut self) -> Result<Value> {
        loop {
            let mut line = String::new();
            if self.stdout.read_line(&mut line)? == 0 {
                bail!("the server closed its output while a response was expected");
            }
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(value) => return Ok(value),
                // Not protocol output. Report it: a server printing to stdout
                // is the most common authoring mistake in any language.
                Err(_) => eprintln!("  note: non-protocol line on stdout: {}", line.trim()),
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Run every case, printing progress. Returns the number that failed.
pub fn run(vectors: &Path, command: &[String]) -> Result<usize> {
    let raw = std::fs::read_to_string(vectors)
        .with_context(|| format!("could not read {}", vectors.display()))?;
    let suite: Suite = serde_json::from_str(&raw)
        .with_context(|| format!("{} is not a conformance suite", vectors.display()))?;

    println!(
        "conformance: {} {} ({} cases) against `{}`",
        suite.protocol,
        suite.version,
        suite.cases.len(),
        command.join(" ")
    );

    let mut server = Server::spawn(command)?;
    let mut failed = 0;

    for case in &suite.cases {
        match run_case(&mut server, case) {
            Ok(()) => println!("  ok    {}", case.name),
            Err(e) => {
                println!("  FAIL  {}: {e}", case.name);
                failed += 1;
            }
        }
    }

    if failed == 0 {
        println!("all {} cases passed", suite.cases.len());
    } else {
        println!("{failed} of {} cases failed", suite.cases.len());
    }
    Ok(failed)
}

fn run_case(server: &mut Server, case: &Case) -> Result<()> {
    server.send(&case.send)?;

    if case.expect.no_response {
        return Ok(());
    }

    let want_id = case.send.get("id").cloned().unwrap_or(Value::Null);
    let mut pending_notifications = case.expect.notifications.clone();

    // Read until the response with our id. Notifications arriving first are
    // matched off in order; anything else is ignored rather than failed, so the
    // suite does not break when an implementation reports more than it must.
    loop {
        let message = server.next_message()?;

        if message.get("method").is_some() && message.get("id").is_none() {
            if let (Some(expected), Some(got)) = (
                pending_notifications.first(),
                message.get("method").and_then(Value::as_str),
            ) && expected == got
            {
                pending_notifications.remove(0);
            }
            continue;
        }

        if message.get("id") != Some(&want_id) {
            continue;
        }

        if !pending_notifications.is_empty() {
            bail!(
                "expected notification `{}` before the response, but it never arrived",
                pending_notifications[0]
            );
        }
        return check(&message, &case.expect);
    }
}

fn check(response: &Value, expect: &Expect) -> Result<()> {
    if let Some(code) = expect.error_code {
        let got = response
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("expected an error with code {code}, got {response}"))?;
        if got != code {
            bail!("expected error code {code}, got {got}");
        }
        return Ok(());
    }

    if let Some(error) = response.get("error") {
        bail!("expected a result, got error {error}");
    }

    if let Some(wanted) = &expect.result_contains {
        let got = response.get("result").unwrap_or(&Value::Null);
        if let Some(mismatch) = subset_mismatch(wanted, got) {
            bail!("{mismatch}");
        }
    }
    Ok(())
}

/// Whether `wanted` is contained in `got`, describing the first difference.
///
/// Objects match on the keys `wanted` names and ignore the rest; everything
/// else matches exactly. That is what keeps the suite forward-compatible.
fn subset_mismatch(wanted: &Value, got: &Value) -> Option<String> {
    match (wanted, got) {
        (Value::Object(wanted), Value::Object(got)) => {
            for (key, value) in wanted {
                match got.get(key) {
                    None => return Some(format!("result is missing `{key}`")),
                    Some(actual) => {
                        if let Some(inner) = subset_mismatch(value, actual) {
                            return Some(format!("at `{key}`: {inner}"));
                        }
                    }
                }
            }
            None
        }
        (wanted, got) if wanted == got => None,
        (wanted, got) => Some(format!("expected {wanted}, got {got}")),
    }
}

/// A starting suite for a protocol that has none: the handshake, and a method
/// that does not exist.
pub fn template(protocol: &str, version: &str) -> Value {
    json!({
        "protocol": protocol,
        "version": version,
        "cases": [
            {
                "name": "handshake reports a compatible version",
                "send": {
                    "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": { "name": "lanok conform", "protocol_version": version }
                },
                "expect": { "result_contains": { "protocol_version": version } }
            },
            {
                "name": "an unknown method is refused, not ignored",
                "send": { "jsonrpc": "2.0", "id": 2, "method": "no/such/method" },
                "expect": { "error_code": -32601 }
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subset_match_ignores_extra_keys() {
        let wanted = json!({ "text": "HI" });
        let got = json!({ "text": "HI", "added_in_a_later_minor": true });
        assert!(subset_mismatch(&wanted, &got).is_none());
    }

    #[test]
    fn a_subset_match_reports_the_first_difference() {
        assert!(
            subset_mismatch(&json!({ "a": 1 }), &json!({}))
                .unwrap()
                .contains("missing `a`")
        );
        assert!(
            subset_mismatch(&json!({ "a": { "b": 1 } }), &json!({ "a": { "b": 2 } }))
                .unwrap()
                .contains("at `a`")
        );
    }

    #[test]
    fn nested_objects_match_by_subset_too() {
        let wanted = json!({ "outer": { "kept": 1 } });
        let got = json!({ "outer": { "kept": 1, "extra": 2 }, "other": 3 });
        assert!(subset_mismatch(&wanted, &got).is_none());
    }
}
