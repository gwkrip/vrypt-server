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

## Overview

Vrypt Server is a minimal, high-performance HTTP server with a single, deliberate purpose — respond with the plain text `Vrypt` to any incoming HTTP request, regardless of method or path.

Built on a **multi-threaded epoll event loop** with `SO_REUSEPORT`, it handles tens of thousands of concurrent connections using only as many OS threads as there are CPU cores — no thread pools, no async runtimes, no unnecessary overhead.

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
                │Thread 0 │ │Thread 1 │ │Thread N │  (N = CPU cores)
                │  epoll  │ │  epoll  │ │  epoll  │
                └─────────┘ └─────────┘ └─────────┘
```

| Design Decision | Benefit |
|---|---|
| **One thread per CPU core** | Perfectly saturates hardware parallelism |
| **SO_REUSEPORT** | Each thread owns its listener socket; the kernel load-balances accepts with zero contention |
| **Non-blocking I/O via epoll** (`mio`) | A single thread manages thousands of concurrent connections |
| **Zero heap allocation per request** | Response is a compile-time static byte slice |
| **TCP_NODELAY** | Nagle's algorithm disabled for minimal latency |
| **Keep-alive support** | Connections are reused, reducing TCP handshake overhead |

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

The release profile is tuned for maximum performance and minimum binary size:

```toml
[profile.release]
opt-level     = 3       # Maximum compiler optimization
lto           = "fat"   # Full link-time optimization across all crates
codegen-units = 1       # Single codegen unit for best inlining
panic         = "abort" # No unwinding overhead
strip         = true    # Strip debug symbols for smallest binary
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
3. Commit your changes (`git commit -m 'feat: add your feature'`)
4. Push to the branch (`git push origin feat/your-feature`)
5. Open a Pull Request

---

## License

This project is licensed under the [MIT License](LICENSE).
