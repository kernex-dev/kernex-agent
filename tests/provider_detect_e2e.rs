//! Which provider the kx binary picks when none is named, end to end.
//!
//! The environment is cleared for each run, so only what a test sets can
//! configure a provider. A small HTTP server stands in for Ollama.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// `kx --no-memory dev hello` in a fresh HOME and project with only `envs`.
fn run_kx(envs: &[(&str, &str)]) -> Run {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let project = home.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_kx"))
        .args(["--no-memory", "dev", "hello"])
        .current_dir(&project)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .envs(envs.iter().copied())
        .output()
        .unwrap();
    Run {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// An Ollama stand-in: answers `/api/tags` and `/api/chat` and records the
/// path of each request. Returns its `host:port`.
fn ollama_stub() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let paths = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&paths);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let request = read_request(&mut stream);
            let path = request
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
            let body = if path == "/api/tags" {
                r#"{"models":[{"name":"stub"}]}"#
            } else {
                r#"{"model":"stub","message":{"role":"assistant","content":"stub-reply"},"done":true}"#
            };
            seen.lock().unwrap().push(path);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (addr, paths)
}

/// The request head and body, read up to its `Content-Length`.
fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf);
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if buf.len() >= end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn a_running_ollama_is_used_and_named() {
    let (addr, paths) = ollama_stub();
    // The compose files pass these empty when .env leaves them unset.
    let run = run_kx(&[
        ("OLLAMA_HOST", addr.as_str()),
        ("KERNEX_PROVIDER", ""),
        ("KERNEX_BASE_URL", ""),
    ]);
    assert!(run.ok, "{}", run.stderr);
    let notice = format!("using provider ollama (Ollama answers at http://{addr})");
    assert!(run.stderr.contains(&notice), "{}", run.stderr);
    assert!(run.stdout.contains("stub-reply"), "{}", run.stdout);
    // The chat went to the address the probe found, not the default one.
    let paths = paths.lock().unwrap().clone();
    assert_eq!(paths.first().map(String::as_str), Some("/api/tags"));
    assert!(paths.iter().any(|p| p == "/api/chat"), "{paths:?}");
}

#[test]
fn nothing_configured_stops_with_the_ways_to_configure_one() {
    let run = run_kx(&[("OLLAMA_HOST", "127.0.0.1:9")]);
    assert!(!run.ok);
    assert!(
        run.stderr.contains("no provider is configured"),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("ANTHROPIC_API_KEY"), "{}", run.stderr);
}

#[test]
fn a_named_provider_skips_the_search() {
    // The provider is named and pointed at one server; the search would
    // probe the other, at OLLAMA_HOST.
    let (probe_addr, probed) = ollama_stub();
    let (named_addr, named) = ollama_stub();
    let run = run_kx(&[
        ("OLLAMA_HOST", probe_addr.as_str()),
        ("KERNEX_PROVIDER", "ollama"),
        ("KERNEX_BASE_URL", &format!("http://{named_addr}")),
    ]);
    assert!(run.ok, "{}", run.stderr);
    assert!(!run.stderr.contains("using provider"), "{}", run.stderr);
    assert!(
        probed.lock().unwrap().is_empty(),
        "kx probed although a provider was named"
    );
    assert!(named.lock().unwrap().iter().any(|p| p == "/api/chat"));
}

#[test]
fn a_named_ollama_finds_its_server_through_ollama_host() {
    let (addr, paths) = ollama_stub();
    let run = run_kx(&[
        ("OLLAMA_HOST", addr.as_str()),
        ("KERNEX_PROVIDER", "ollama"),
    ]);
    assert!(run.ok, "{}", run.stderr);
    assert!(!run.stderr.contains("using provider"), "{}", run.stderr);
    assert!(run.stdout.contains("stub-reply"), "{}", run.stdout);
    assert!(paths.lock().unwrap().iter().any(|p| p == "/api/chat"));
}

#[test]
fn commands_that_call_no_model_do_not_look() {
    for args in [["skills", "list"], ["pipeline", "list"], ["cron", "list"]] {
        let root = tempfile::tempdir().unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_kx"))
            .args(args)
            .current_dir(root.path())
            .env_clear()
            .env("HOME", root.path())
            .env("PATH", "/usr/bin:/bin")
            .env("OLLAMA_HOST", "127.0.0.1:9")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{args:?}: {stderr}");
        assert!(!stderr.contains("provider"), "{args:?}: {stderr}");
    }
}
