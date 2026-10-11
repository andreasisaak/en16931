//! Runs hostile-input tests in a watched child process (this test binary,
//! filtered to one test), killed past [`MAX_RSS_BYTES`] or [`MAX_WALL`], so a
//! broken bound fails the test instead of taking the machine. [`watch`] does
//! the same for the command-line suite. Memory is read with `ps` on Unix;
//! elsewhere only the time cap applies.

#![allow(dead_code)]

use std::io::Read as _;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// The most resident memory a watched process may reach.
pub const MAX_RSS_BYTES: u64 = 512 * 1024 * 1024;

/// The longest a watched process may run.
pub const MAX_WALL: Duration = Duration::from_secs(10);

/// Set in the child to the name of the test it is to run in-process.
const CHILD: &str = "EN16931_WATCHDOG_CHILD";

/// Run `body` — the test named `test` — in a watched child process.
///
/// The parent re-executes the test binary with `test --exact` under the caps;
/// the child (recognised by an environment variable) runs `body`.
///
/// # Panics
///
/// When the child fails, exceeds a cap, or ran no test at all (a `test` that
/// does not name the calling test).
pub fn isolated(test: &str, body: impl FnOnce()) {
    if std::env::var(CHILD).as_deref() == Ok(test) {
        body();
        return;
    }
    let exe = std::env::current_exe().expect("the test binary's own path");
    let mut cmd = Command::new(exe);
    cmd.args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, test);
    let out = watch(cmd, &format!("test {test}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "{test} failed in its child process:\n{text}"
    );
    assert!(
        text.contains("1 passed"),
        "the child ran no test named {test:?}:\n{text}"
    );
}

/// Run `cmd` to completion with its output captured, killing it — and
/// panicking with the reason — past [`MAX_RSS_BYTES`] or [`MAX_WALL`].
pub fn watch(mut cmd: Command, what: &str) -> Output {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap_or_else(|e| panic!("{what}: spawn: {e}"));
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        let rss = rss_bytes(child.id());
        let over = if rss > MAX_RSS_BYTES {
            Some(format!(
                "{} MiB resident (cap {} MiB)",
                rss >> 20,
                MAX_RSS_BYTES >> 20
            ))
        } else if start.elapsed() > MAX_WALL {
            Some(format!("{:?} running (cap {MAX_WALL:?})", start.elapsed()))
        } else {
            None
        };
        if let Some(why) = over {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what}: killed by the watchdog at {why} — a bound is broken");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    }
}

/// The resident set of `pid` in bytes; 0 when it cannot be read.
#[cfg(unix)]
fn rss_bytes(pid: u32) -> u64 {
    Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map_or(0, |kib| kib * 1024)
}

#[cfg(not(unix))]
fn rss_bytes(_pid: u32) -> u64 {
    0
}
