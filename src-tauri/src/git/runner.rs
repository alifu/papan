//! Runs the system `git` with a fixed argument vector. Never a shell.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::AppError;

pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
/// Per stream; a larger diff is cut with a marker.
pub const MAX_OUTPUT: usize = 1024 * 1024;
const POLL: Duration = Duration::from_millis(20);
/// How long to wait for output after git exited; a grandchild may still hold the pipe.
const DRAIN_GRACE: Duration = Duration::from_secs(2);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A path that is not a hooks directory, so no hook of a shared repository runs.
#[cfg(unix)]
const NO_HOOKS: &str = "core.hooksPath=/dev/null";
#[cfg(windows)]
const NO_HOOKS: &str = "core.hooksPath=NUL";

/// Options on every call: only the transports Papan needs, no hooks, no fsmonitor, so no
/// program named by a repository's own config runs on status, diff or commit.
const SAFE_OPTIONS: [&str; 16] = [
    "-c",
    "protocol.allow=never",
    "-c",
    "protocol.https.allow=always",
    "-c",
    "protocol.ssh.allow=always",
    "-c",
    "protocol.git.allow=always",
    "-c",
    "protocol.file.allow=always",
    "-c",
    "core.fsmonitor=false",
    "-c",
    NO_HOOKS,
    "-c",
    "core.pager=cat",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRun {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Replaces `user:password@` (or `token@`) in any URL with `***@`.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c == '/' || c.is_whitespace())
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(pos) => {
                out.push_str("***");
                rest = &tail[pos..];
            }
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

/// A value that becomes an argument must not be read by git as an option.
pub fn reject_option_like(what: &str, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::invalid_input(format!("{what} is required.")));
    }
    if value.starts_with('-') || value.contains('\0') || value.contains('\n') {
        return Err(AppError::invalid_input(format!("{what} is not valid.")));
    }
    Ok(())
}

fn read_capped(mut stream: impl Read) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = MAX_OUTPUT.saturating_sub(buffer.len());
                if n > room {
                    buffer.extend_from_slice(&chunk[..room]);
                    truncated = true; // keep draining so the child never blocks on a full pipe
                } else {
                    buffer.extend_from_slice(&chunk[..n]);
                }
            }
        }
    }
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if truncated {
        text.push_str("\n… output truncated");
    }
    text
}

/// Reads a stream on its own thread; the text arrives on the returned channel.
fn spawn_reader(stream: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(read_capped(stream));
    });
    rx
}

/// Stops git and, on Unix, everything it started (ssh helpers, credential daemons).
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: the child was started as leader of its own process group
            // (`process_group(0)`), so `-pid` names only that group.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .args(SAFE_OPTIONS)
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, CREATE_NO_WINDOW);
    command
}

/// Runs `git <args>` in `dir`. A non-zero exit is `Ok` with `success: false`; failing to
/// start git or hitting the timeout is an error.
pub fn run(dir: &Path, args: &[&str], timeout: Duration) -> Result<GitRun, AppError> {
    let mut child = command(dir, args).spawn().map_err(|source| {
        AppError::not_found("Git could not be started. Is it installed?")
            .with_detail(source.to_string())
    })?;
    let stdout = child.stdout.take().map(spawn_reader);
    let stderr = child.stderr.take().map(spawn_reader);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                kill_tree(&mut child);
                return Err(AppError::new(
                    crate::error::ErrorCode::Timeout,
                    "Git took too long and was stopped.",
                ));
            }
            Ok(None) => thread::sleep(POLL),
            Err(source) => {
                return Err(
                    AppError::internal("Git stopped unexpectedly.").with_detail(source.to_string())
                )
            }
        }
    };
    let drain = |rx: Option<mpsc::Receiver<String>>| {
        rx.and_then(|rx| rx.recv_timeout(DRAIN_GRACE).ok())
            .unwrap_or_default()
    };
    Ok(GitRun {
        success: status.success(),
        stdout: drain(stdout),
        stderr: redact(&drain(stderr)),
    })
}

/// Like [`run`] but a non-zero exit becomes an error carrying git's message.
pub fn run_ok(dir: &Path, args: &[&str], timeout: Duration) -> Result<GitRun, AppError> {
    let result = run(dir, args, timeout)?;
    if result.success {
        return Ok(result);
    }
    let message = result.stderr.trim();
    let message = if message.is_empty() {
        result.stdout.trim()
    } else {
        message
    };
    Err(AppError::invalid_input(redact(message))
        .with_detail(redact(&format!("git {}", args.join(" ")))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_in_urls() {
        let text = "fatal: unable to access 'https://user:tok@github.com/a/b.git/': 403";
        assert_eq!(
            redact(text),
            "fatal: unable to access 'https://***@github.com/a/b.git/': 403"
        );
        assert_eq!(redact("https://github.com/a/b"), "https://github.com/a/b");
        assert_eq!(redact("git@github.com:a/b.git"), "git@github.com:a/b.git");
    }

    #[test]
    fn refuses_option_like_values() {
        assert!(reject_option_like("URL", "--upload-pack=x").is_err());
        assert!(reject_option_like("URL", "  ").is_err());
        assert!(reject_option_like("URL", "a\nb").is_err());
        assert!(reject_option_like("URL", "https://x/y").is_ok());
    }

    #[test]
    fn reports_failure_and_stops_on_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let failed = run(dir.path(), &["status"], LOCAL_TIMEOUT).unwrap();
        assert!(!failed.success);
        assert!(run_ok(dir.path(), &["status"], LOCAL_TIMEOUT).is_err());
        let slow = run(
            dir.path(),
            &["-c", "alias.nap=!sleep 5", "nap"],
            Duration::from_millis(100),
        );
        assert_eq!(slow.unwrap_err().code, crate::error::ErrorCode::Timeout);
    }
}
