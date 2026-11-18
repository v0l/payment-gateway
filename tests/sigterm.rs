mod common;

use common::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn spawn_gateway(db_url: &str, port: u16, extra: &[(&str, &str)]) -> std::process::Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_payment-gateway"));
    cmd.env_clear()
        .env("DATABASE_URL", db_url)
        .env("GATEWAY_ENCRYPTION_KEY", "09".repeat(32))
        .env("GATEWAY_ADMIN_TOKEN", ADMIN)
        .env("GATEWAY_PUBLIC_URL", "https://pay.test")
        .env("GATEWAY_LISTEN", format!("127.0.0.1:{port}"))
        .env("WORKER_TICK_SECS", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.spawn().unwrap()
}

fn wait_ready(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            s.write_all(b"GET /ready HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n")
                .unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            if out.starts_with("HTTP/1.1 200") {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("gateway did not become ready");
}

fn db_url(h: &Harness) -> String {
    let base = std::env::var("TEST_DATABASE_URL").unwrap();
    let (prefix, _) = base.rsplit_once('/').unwrap();
    let name: (String,) =
        futures::executor::block_on(sqlx::query_as("select database()").fetch_one(&h.pool))
            .unwrap();
    format!("{prefix}/{}", name.0)
}

#[tokio::test(flavor = "multi_thread")]
async fn sigterm_lets_an_in_flight_request_finish_then_exits_cleanly() {
    let h = harness().await;
    let url = db_url(&h);
    let port = free_port();
    let mut child = spawn_gateway(&url, port, &[("LOG_LEVEL", "info")]);
    tokio::task::spawn_blocking(move || wait_ready(port))
        .await
        .unwrap();

    let body = r#"{"name":"sigterm-tenant"}"#;
    let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let head = format!(
        "POST /admin/tenants HTTP/1.1\r\nhost: x\r\nauthorization: Bearer {ADMIN}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    conn.write_all(head.as_bytes()).unwrap();
    conn.write_all(&body.as_bytes()[..5]).unwrap();
    conn.flush().unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let pid = child.id().to_string();
    assert!(
        Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .unwrap()
            .success()
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        child.try_wait().unwrap().is_none(),
        "exited with a request in flight"
    );

    conn.write_all(&body.as_bytes()[5..]).unwrap();
    let mut out = String::new();
    conn.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
    assert!(out.contains("\"api_key\""), "{out}");

    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "did not exit after SIGTERM");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(status.success(), "{status:?}");

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.contains("shutdown requested"), "{stderr}");
    let leases: (i64,) = sqlx::query_as("select count(*) from worker_lease")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(leases.0, 0, "lease not released");
    let tenants: (i64,) =
        sqlx::query_as("select count(*) from tenant where name = 'sigterm-tenant'")
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(tenants.0, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_config_exits_with_every_error_listed() {
    let out = Command::new(env!("CARGO_BIN_EXE_payment-gateway"))
        .env_clear()
        .env("GATEWAY_ADMIN_TOKEN", "short")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("DATABASE_URL is required"), "{err}");
    assert!(err.contains("GATEWAY_ENCRYPTION_KEY is required"), "{err}");
    assert!(
        err.contains("GATEWAY_ADMIN_TOKEN must be at least 24"),
        "{err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn binary_logs_json_with_request_id_and_no_secrets() {
    let h = harness().await;
    let url = db_url(&h);
    let port = free_port();
    let mut child = spawn_gateway(&url, port, &[("LOG_LEVEL", "debug")]);
    tokio::task::spawn_blocking(move || wait_ready(port))
        .await
        .unwrap();
    let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
    conn.write_all(
        format!("GET /admin/tenants HTTP/1.1\r\nhost: x\r\nx-request-id: rid-bin-9\r\nauthorization: Bearer {ADMIN}\r\nconnection: close\r\n\r\n").as_bytes(),
    )
    .unwrap();
    let mut out = String::new();
    conn.read_to_string(&mut out).unwrap();
    assert!(
        out.to_lowercase().contains("x-request-id: rid-bin-9"),
        "{out}"
    );
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let _ = child.wait();
    let line = stderr
        .lines()
        .find(|l| l.contains("rid-bin-9"))
        .unwrap_or_else(|| panic!("no request log: {stderr}"));
    let v: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(v["request_id"], "rid-bin-9");
    assert!(!stderr.contains(ADMIN));
}
