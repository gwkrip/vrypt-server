mod config;
mod conn;
mod counter;
mod pool;
mod slab;
mod timer;
mod worker;

use config::{DEFAULT_PORT, RESPONSE_BODY, STATS_INTERVAL, STATS_TARGET};
use counter::{RpsCounter, spawn_stats_pusher};
use worker::worker;
use std::net::SocketAddr;
use std::thread;

fn build_response(body: &[u8]) -> Vec<u8> {
    let mut res = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    )
    .into_bytes();
    res.extend_from_slice(body);
    res
}

fn parse_args() -> SocketAddr {
    let mut args = std::env::args().skip(1);
    let port = match args.next().as_deref() {
        Some("-p") | Some("--port") => args
            .next()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or_else(|| {
                eprintln!("Invalid port, using default {DEFAULT_PORT}");
                DEFAULT_PORT
            }),
        Some(v) => v.parse::<u16>().unwrap_or_else(|_| {
            eprintln!("Invalid port '{v}', using default {DEFAULT_PORT}");
            DEFAULT_PORT
        }),
        None => DEFAULT_PORT,
    };
    format!("0.0.0.0:{port}").parse().unwrap()
}

fn main() {
    let addr = parse_args();
    let cpus = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let response: &'static [u8] = Box::leak(build_response(RESPONSE_BODY).into_boxed_slice());
    let counter: &'static RpsCounter = RpsCounter::new(cpus);

    spawn_stats_pusher(counter);

    println!("Vrypt listening on {addr} ({cpus} threads)");
    println!("Stats pushing to {STATS_TARGET} every {}s", STATS_INTERVAL.as_secs());

    let handles: Vec<_> = (0..cpus)
        .map(|i| thread::spawn(move || worker(addr, response, counter, i)))
        .collect();

    for h in handles {
        if let Err(e) = h.join() {
            eprintln!("[error] thread panic: {e:?}");
        }
    }
}
