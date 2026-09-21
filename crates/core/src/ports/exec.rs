use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// A single external command invocation, captured so that thin command
/// wrappers stay testable.
pub trait Exec {
    fn run(&self, program: &str, args: &[&str]) -> Result<String>;

    /// Like [`Exec::run`], but feeds `stdin` to the child process instead of
    /// leaving it unset.
    ///
    /// This exists for commands like `ssh-keygen -lf -` that read their
    /// input from stdin, and matters most when that input is a secret: a
    /// value that only ever touches a pipe never appears in `ps` output or a
    /// command log, which a temp file or an argument would not guarantee.
    fn run_with_stdin(&self, program: &str, args: &[&str], stdin: &str) -> Result<String>;
}

pub struct RealExec;

impl Exec for RealExec {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| Error::Command {
                cmd: program.to_string(),
                stderr: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(Error::Command {
                cmd: format!("{program} {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    // Order matters here, and must not be "simplified" back to writing then
    // propagating that error with `?`: a write can fail on its own (EPIPE,
    // when the child exits before reading all of it — exactly what happens
    // when `ssh-keygen` rejects its input) while the child process is still
    // alive and unreaped. Returning early in that case would drop `child`
    // un-waited, leaking a zombie — harmless once, but this runs from a
    // long-lived menubar app where it adds up. So: write, drop stdin (so the
    // child sees EOF even if the write never got to), *then* always wait.
    //
    // This also writes the whole payload before reading anything the child
    // produced. For the one line in and one line out this method is used
    // for today, that is far below the pipe buffer (~64KB on macOS) and
    // safe. A caller pushing more than that through here would deadlock —
    // the child blocks writing to a full stdout pipe while we're still
    // blocked writing its stdin — and would need a threaded or async drain
    // instead of this straight-line write-then-wait.
    fn run_with_stdin(&self, program: &str, args: &[&str], stdin: &str) -> Result<String> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Command {
                cmd: program.to_string(),
                stderr: e.to_string(),
            })?;

        let write_result = child
            .stdin
            .as_mut()
            .expect("stdin was requested as piped")
            .write_all(stdin.as_bytes());
        // Close our end of stdin regardless of whether the write succeeded,
        // so the child sees EOF instead of blocking on more input that will
        // never come.
        drop(child.stdin.take());

        let output = child.wait_with_output().map_err(|e| Error::Command {
            cmd: program.to_string(),
            stderr: e.to_string(),
        })?;

        let child_stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        match (write_result, output.status.success()) {
            (Ok(()), true) => Ok(String::from_utf8_lossy(&output.stdout).to_string()),
            (Ok(()), false) => Err(Error::Command {
                cmd: format!("{program} {}", args.join(" ")),
                stderr: child_stderr,
            }),
            (Err(write_err), success) => {
                // The write error never carries the payload we tried to
                // send — only what the OS reported about the pipe (e.g.
                // "Broken pipe (os error 32)") — so it is safe to include.
                // When the child also failed and left its own stderr, that
                // is the more useful message (it is why the child stopped
                // reading), so prefer it. Only fall back to the write error
                // — rather than discarding it — when the child said nothing
                // useful, or didn't fail at all.
                let stderr = if !success && !child_stderr.is_empty() {
                    child_stderr
                } else {
                    format!("writing to stdin failed: {write_err}")
                };
                Err(Error::Command {
                    cmd: format!("{program} {}", args.join(" ")),
                    stderr,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `run_with_stdin` must reap the child even when writing to its stdin
    /// fails, and must never let the stdin payload leak into the reported
    /// error. This is not exercised through `FakeExec`, which never spawns a
    /// process, so it runs `RealExec` against a real short-lived command —
    /// the same pattern `RealGit`'s tests use elsewhere in this crate.
    ///
    /// `true` exits almost immediately without ever reading stdin. A payload
    /// larger than the OS pipe buffer is not a timing race: `write_all`
    /// blocks once the buffer fills, and unblocks only when the reader
    /// either drains it (never happens here) or closes its end (`true`
    /// exiting) — which reliably delivers a broken-pipe error on the write.
    #[test]
    fn a_stdin_write_past_a_reader_that_already_exited_fails_without_leaking_the_payload() {
        let payload = format!("TOKEN_SHOULD_NEVER_LEAK_{}", "x".repeat(5_000_000));

        let err = RealExec
            .run_with_stdin("true", &[], &payload)
            .expect_err("writing past a closed pipe must fail, not succeed");

        let msg = err.to_string();
        assert!(
            msg.contains("writing to stdin failed"),
            "expected a write-stage error, got: {msg}"
        );
        assert!(
            !msg.contains("TOKEN_SHOULD_NEVER_LEAK"),
            "the stdin payload must never appear in an error message: {msg}"
        );
    }
}
