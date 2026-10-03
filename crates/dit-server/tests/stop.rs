//! Stopping the server (ADR 0029). A write is a commit; a server killed in
//! the middle of one leaves `.git/index.lock` behind and every later write
//! failing. So a stop refuses new connections and lets requests in flight
//! finish — and a connection that never finishes (live updates) cannot hold
//! the process open past the grace period.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use axum::routing::get;

fn free_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    probe.local_addr().unwrap().port()
}

/// GET `path` on a plain socket, from a thread: the answer, or "" when the
/// connection was cut.
fn get_in_thread(port: u16, path: &'static str) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_in_flight_when_the_stop_arrives_completes() {
    let app = axum::Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(400)).await;
            "written"
        }),
    );
    let port = free_port();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (bound_tx, bound_rx) = std::sync::mpsc::channel();
    let server = tokio::spawn(dit_server::serve_until(
        app,
        "127.0.0.1".to_owned(),
        port,
        move || bound_tx.send(()).unwrap(),
        async move {
            let _ = stop_rx.await;
        },
        Duration::from_secs(5),
    ));
    bound_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let answer = get_in_thread(port, "/slow");
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop_tx.send(()).unwrap();

    let answer = tokio::task::spawn_blocking(move || answer.join().unwrap())
        .await
        .unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.ends_with("written"), "{answer}");
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("the server stops once the request is answered")
        .unwrap()
        .unwrap();
    // And it no longer accepts anything.
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_that_never_ends_cannot_hold_the_stop_past_the_grace_period() {
    let app = axum::Router::new().route(
        "/forever",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            "never"
        }),
    );
    let port = free_port();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (bound_tx, bound_rx) = std::sync::mpsc::channel();
    let server = tokio::spawn(dit_server::serve_until(
        app,
        "127.0.0.1".to_owned(),
        port,
        move || bound_tx.send(()).unwrap(),
        async move {
            let _ = stop_rx.await;
        },
        Duration::from_millis(300),
    ));
    bound_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let _hanging = get_in_thread(port, "/forever");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let started = Instant::now();
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("the grace period bounds the stop")
        .unwrap()
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}
