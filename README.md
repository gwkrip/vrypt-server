<div align="center">

# ⚡ Vrypt Server

**A blazing-fast, minimal HTTP server written in Rust.**  
Responds `Vrypt` to every method, every path, every time.

[![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/gwkrip/vrypt-server?logo=github)](https://github.com/gwkrip/vrypt-server/releases)
[![Build](https://img.shields.io/github/actions/workflow/status/gwkrip/vrypt-server/release.yml?logo=github-actions&label=build)](https://github.com/gwkrip/vrypt-server/actions)

</div>

---

## Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Architecture](#architecture)
- [Project Structure](#project-structure)
- [Memory Model](#memory-model)
- [RPS Metrics](#rps-metrics)
- [Installation](#installation)
- [Usage](#usage)
- [Build Profiles](#build-profiles)
- [Dependencies](#dependencies)
- [Supported Targets](#supported-targets)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

Vrypt Server is a minimal, high-performance HTTP server with a single, deliberate purpose — respond with the plain text `Vrypt` to any incoming HTTP request, regardless of method or path.

Built on a **multi-threaded epoll event loop** with `SO_REUSEPORT`, it handles tens of thousands of concurrent connections using only as many OS threads as there are CPU cores — no thread pools, no async runtimes, no unnecessary overhead.

---

## Features

- **Zero-overhead response** — compile-time static byte slice, no heap allocation per request
- **Multi-threaded epoll** — one thread per CPU core, fully saturating hardware parallelism
- **`SO_REUSEPORT` kernel dispatch** — each thread owns its listener socket with zero accept contention
- **Lazy buffer pool** — memory allocated on demand, recycled on close, ~0 MB at idle
- **Sharded RPS counter** — per-thread atomic slots with cache-line padding, no false sharing
- **Real-time StatsD metrics** — RPS pushed via UDP every second, fire-and-forget
- **`TCP_NODELAY`** — Nagle's algorithm disabled for minimal latency
- **HTTP Keep-Alive** — connection reuse to reduce TCP handshake overhead

---

## Architecture

```
                    ┌─────────────────────────────────┐
                    │         Incoming Requests        │
                    └────────────────┬────────────────┘
                                     │
                    ┌────────────────▼────────────────┐
                    │   Kernel (SO_REUSEPORT dispatch) │
                    └──┬──────────┬──────────┬────────┘
                       │          │          │
                ┌──────▼──┐ ┌─────▼───┐ ┌───▼─────┐
                │Thread 0 │ │Thread 1 │ │Thread N │  ← N = CPU cores
                │  epoll  │ │  epoll  │ │  epoll  │
                └────┬────┘ └────┬────┘ └────┬────┘
                     │           │            │
                ┌────▼───────────▼────────────▼────┐
                │       Sharded RPS Counter         │
                │  (per-thread, cache-line padded)  │
                └────────────────┬─────────────────┘
                                 │  UDP push / 1s
                    ┌────────────▼────────────┐
                    │     StatsD Collector     │
                    │     127.0.0.1:8125       │
                    └─────────────────────────┘
```

### Design Decisions

| Decision | Benefit |
|---|---|
| **One thread per CPU core** | Perfectly saturates hardware parallelism |
| **`SO_REUSEPORT`** | Each thread owns its listener socket; the kernel load-balances accepts with zero contention |
| **Non-blocking I/O via epoll** (`mio`) | A single thread manages thousands of concurrent connections |
| **Lazy buffer pool** | Memory allocated only when connections arrive, recycled on close — ~0 MB at idle |
| **Sharded RPS counter** | Per-thread atomic slots with cache-line padding eliminate false sharing |
| **UDP stats push** | RPS metrics sent to StatsD every second — fire-and-forget, zero blocking |
| **Zero heap allocation per request** | Response is a compile-time static byte slice |
| **`TCP_NODELAY`** | Nagle's algorithm disabled for minimal latency |
| **Keep-alive support** | Connections are reused, reducing TCP handshake overhead |

---

## Project Structure

```
vrypt/
├── Cargo.toml
└── src/
    ├── main.rs      — entry point, argument parsing, response builder
    ├── config.rs    — all constants and tuning parameters
    ├── conn.rs      — Conn struct and per-connection state
    ├── counter.rs   — sharded RPS counter + UDP stats pusher
    ├── pool.rs      — BufPool (lazy) and TokenPool
    ├── slab.rs      — fixed-size connection slab allocator
    └── worker.rs    — epoll event loop and I/O handlers
```

---

## Memory Model

The buffer pool uses **lazy allocation with capped recycling**. No memory is pre-allocated at startup — buffers are created on demand and returned to a recycle list on connection close. When the recycle list is full, excess buffers are dropped and returned to the OS immediately.

| State | Memory per Worker |
|---|---|
| Idle (0 connections) | ~0 MB |
| 256 active connections | ~16 MB |
| Full load (65,536 connections) | ~4 GB |

---

## RPS Metrics

Vrypt pushes real-time request-per-second metrics via **UDP in StatsD gauge format** to `127.0.0.1:8125` every second.

```
vrypt.rps:12345|g
```

The counter uses **per-thread atomic slots** padded to 64 bytes (one cache line each), so worker threads never contend with each other when incrementing. A dedicated stats thread aggregates all slots and sends the UDP datagram — completely isolated from the hot path.

**Listen for metrics locally (for testing):**

```bash
nc -u -l 8125
```

**Configure the target collector or push interval** in `src/config.rs`:

```rust
pub const STATS_TARGET: &str = "127.0.0.1:8125";
pub const STATS_INTERVAL: Duration = Duration::from_secs(1);
```

---

## Installation

### Option 1 — Pre-built Binaries

Download the latest binary for your platform from the [Releases](https://github.com/gwkrip/vrypt-server/releases) page.

| Platform | File |
|---|---|
| Linux x64 (glibc) | `vrypt-server-*-x86_64-unknown-linux-gnu.tar.gz` |
| Linux x64 (musl / Alpine) | `vrypt-server-*-x86_64-unknown-linux-musl.tar.gz` |
| Linux x86 (glibc) | `vrypt-server-*-i686-unknown-linux-gnu.tar.gz` |
| Linux x86 (musl) | `vrypt-server-*-i686-unknown-linux-musl.tar.gz` |
| Linux ARM64 (glibc) | `vrypt-server-*-aarch64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 (musl) | `vrypt-server-*-aarch64-unknown-linux-musl.tar.gz` |
| Linux ARMv7 (glibc) | `vrypt-server-*-armv7-unknown-linux-gnueabihf.tar.gz` |
| Linux ARMv7 (musl) | `vrypt-server-*-armv7-unknown-linux-musleabihf.tar.gz` |

```bash
tar -xzf vrypt-server-*-<target>.tar.gz
chmod +x vrypt-server
./vrypt-server
```

### Option 2 — Build from Source

**Prerequisites:** [Rust stable](https://rustup.rs)

```bash
git clone https://github.com/gwkrip/vrypt-server.git
cd vrypt-server

# Development build
cargo run

# Production build (fully optimized)
cargo build --release
./target/release/vrypt-server
```

---

## Usage

```bash
# Start on the default port (8080)
./vrypt-server

# Specify a custom port via flag
./vrypt-server --port 3000
./vrypt-server -p 3000

# Specify a custom port positionally
./vrypt-server 3000
```

### Verify It's Working

```bash
curl http://localhost:8080/
# → Vrypt

curl -X POST http://localhost:8080/any/path
# → Vrypt

curl -X DELETE http://localhost:8080/foo/bar/baz
# → Vrypt
```

---

## Build Profiles

The release profile is tuned for maximum performance:

```toml
[profile.release]
opt-level     = 3        # Maximum compiler optimization
lto           = "thin"   # Link-time optimization across crates
codegen-units = 1        # Single codegen unit for best inlining
```

---

## Dependencies

Vrypt Server keeps its dependency footprint intentionally minimal:

| Crate | Purpose |
|---|---|
| [`mio`](https://crates.io/crates/mio) | Cross-platform epoll / kqueue abstraction |
| [`socket2`](https://crates.io/crates/socket2) | Low-level socket configuration (`SO_REUSEPORT`) |

No async runtime. No HTTP framework. Just the essentials.

---

## Supported Targets

8 pre-built binaries are published on every release, covering all major Linux architectures:

| Architecture | glibc | musl (static) |
|---|---|---|
| x86_64  | `x86_64-unknown-linux-gnu`      | `x86_64-unknown-linux-musl`      |
| i686    | `i686-unknown-linux-gnu`        | `i686-unknown-linux-musl`        |
| aarch64 | `aarch64-unknown-linux-gnu`     | `aarch64-unknown-linux-musl`     |
| armv7   | `armv7-unknown-linux-gnueabihf` | `armv7-unknown-linux-musleabihf` |

> **musl builds** are fully statically linked — ideal for Alpine Linux, containers, and environments without a system libc.

---

## Contributing

Contributions, issues, and feature requests are welcome. Please open an issue first to discuss what you would like to change.

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/your-feature`)
3. Commit your changes using [Conventional Commits](https://www.conventionalcommits.org/) (`git commit -m 'feat: add your feature'`)
4. Push to the branch (`git push origin feat/your-feature`)
5. Open a Pull Request

---

## License

This project is licensed under the [MIT License](LICENSE).

---

<div align="center">

Made with ❤️ and Rust

</div>
