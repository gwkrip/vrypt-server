use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use socket2::{Domain, Protocol, Socket, Type};
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::thread;
use std::time::{Duration, Instant};

const SERVER_TOKEN: Token = Token(0);
const DEFAULT_PORT: u16 = 8080;
const CONN_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_TIMEOUT: Duration = Duration::from_millis(5000);
const MAX_REQUEST_SIZE: usize = 64 * 1024;
const RESPONSE_BODY: &[u8] = b"Vrypt";
const MAX_CONNS: usize = 65536;
const BUF_SIZE: usize = 64 * 1024;
const MAX_RECYCLED_BUFS: usize = 256;

struct BufPool {
    free: Vec<Box<[u8; BUF_SIZE]>>,
    active: usize,
    max_active: usize,
    max_recycled: usize,
}

impl BufPool {
    fn new(max_active: usize, max_recycled: usize) -> Self {
        Self {
            free: Vec::with_capacity(max_recycled),
            active: 0,
            max_active,
            max_recycled,
        }
    }

    #[inline]
    fn acquire(&mut self) -> Option<Box<[u8; BUF_SIZE]>> {
        if self.active >= self.max_active {
            return None;
        }
        self.active += 1;
        Some(self.free.pop().unwrap_or_else(|| Box::new([0u8; BUF_SIZE])))
    }

    #[inline]
    fn release(&mut self, mut buf: Box<[u8; BUF_SIZE]>) {
        self.active = self.active.saturating_sub(1);
        if self.free.len() < self.max_recycled {
            buf.fill(0);
            self.free.push(buf);
        }
    }
}

struct TokenPool {
    next: usize,
    free: Vec<usize>,
}

impl TokenPool {
    fn new() -> Self {
        Self { next: 1, free: Vec::with_capacity(MAX_CONNS) }
    }

    #[inline]
    fn acquire(&mut self) -> Option<Token> {
        if let Some(t) = self.free.pop() {
            return Some(Token(t));
        }
        if self.next < MAX_CONNS {
            let t = self.next;
            self.next += 1;
            Some(Token(t))
        } else {
            None
        }
    }

    #[inline]
    fn release(&mut self, tok: Token) {
        self.free.push(tok.0);
    }
}

struct Conn {
    stream: mio::net::TcpStream,
    read_buf: Box<[u8; BUF_SIZE]>,
    read_len: usize,
    write_buf: &'static [u8],
    write_pos: usize,
    last_active: Instant,
}

impl Conn {
    fn new(stream: mio::net::TcpStream, response: &'static [u8], buf: Box<[u8; BUF_SIZE]>) -> Self {
        Self {
            stream,
            read_buf: buf,
            read_len: 0,
            write_buf: response,
            write_pos: usize::MAX,
            last_active: Instant::now(),
        }
    }

    #[inline]
    fn filled(&self) -> &[u8] {
        &self.read_buf[..self.read_len]
    }

    #[inline]
    fn request_complete(&self) -> bool {
        self.filled().windows(4).any(|w| w == b"\r\n\r\n")
    }

    #[inline]
    fn has_pending_write(&self) -> bool {
        self.write_pos < self.write_buf.len()
    }

    #[inline]
    fn arm_write(&mut self) {
        self.read_len = 0;
        self.write_pos = 0;
    }

    #[inline]
    fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    #[inline]
    fn is_timed_out(&self) -> bool {
        self.last_active.elapsed() > CONN_TIMEOUT
    }
}

struct Slab {
    slots: Vec<Option<Conn>>,
}

impl Slab {
    fn new(cap: usize) -> Self {
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(None);
        }
        Self { slots }
    }

    #[inline]
    fn insert(&mut self, tok: Token, conn: Conn) {
        self.slots[tok.0] = Some(conn);
    }

    #[inline]
    fn get_mut(&mut self, tok: Token) -> Option<&mut Conn> {
        self.slots[tok.0].as_mut()
    }

    fn iter_tokens(&self) -> impl Iterator<Item = Token> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|_| Token(i)))
    }
}

fn build_response(body: &[u8]) -> Vec<u8> {
    let mut res = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    )
    .into_bytes();
    res.extend_from_slice(body);
    res
}

fn worker(addr: SocketAddr, response: &'static [u8]) {
    let sock = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).expect("socket::new");
    sock.set_reuse_address(true).expect("set_reuse_address");
    sock.set_reuse_port(true).expect("set_reuse_port");
    sock.set_nonblocking(true).expect("set_nonblocking");
    sock.bind(&addr.into()).expect("bind");
    sock.listen(4096).expect("listen");

    let std_listener = unsafe { std::net::TcpListener::from_raw_fd(sock.into_raw_fd()) };
    let mut listener = TcpListener::from_std(std_listener);

    let mut poll = Poll::new().expect("poll::new");
    let mut events = Events::with_capacity(1024);
    let mut slab = Slab::new(MAX_CONNS);
    let mut buf_pool = BufPool::new(MAX_CONNS, MAX_RECYCLED_BUFS);
    let mut token_pool = TokenPool::new();
    let mut to_close: Vec<Token> = Vec::with_capacity(MAX_CONNS);
    let mut timed_out: Vec<Token> = Vec::with_capacity(MAX_CONNS);

    poll.registry()
        .register(&mut listener, SERVER_TOKEN, Interest::READABLE)
        .expect("register listener");

    loop {
        loop {
            match poll.poll(&mut events, Some(POLL_TIMEOUT)) {
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => panic!("poll: {e}"),
            }
        }

        to_close.clear();

        for event in events.iter() {
            match event.token() {
                SERVER_TOKEN => {
                    accept_connections(
                        &mut listener,
                        &mut slab,
                        &mut token_pool,
                        &mut buf_pool,
                        &poll,
                        response,
                    );
                }
                token => {
                    handle_connection(token, &mut slab, &poll, &mut to_close);
                }
            }
        }

        for tok in to_close.drain(..) {
            close_conn(&mut slab, &mut token_pool, &mut buf_pool, &poll, tok);
        }

        timed_out.clear();
        for tok in slab.iter_tokens() {
            if slab.slots[tok.0].as_ref().map_or(false, |c| c.is_timed_out()) {
                timed_out.push(tok);
            }
        }

        for tok in timed_out.drain(..) {
            eprintln!("[info] timeout, closing {:?}", tok);
            close_conn(&mut slab, &mut token_pool, &mut buf_pool, &poll, tok);
        }
    }
}

fn accept_connections(
    listener: &mut TcpListener,
    slab: &mut Slab,
    token_pool: &mut TokenPool,
    buf_pool: &mut BufPool,
    poll: &Poll,
    response: &'static [u8],
) {
    loop {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let _ = stream.set_nodelay(true);

                let tok = match token_pool.acquire() {
                    Some(t) => t,
                    None => {
                        eprintln!("[warn] token pool exhausted, dropping connection");
                        continue;
                    }
                };
                let buf = match buf_pool.acquire() {
                    Some(b) => b,
                    None => {
                        eprintln!("[warn] buffer pool exhausted, dropping connection");
                        token_pool.release(tok);
                        continue;
                    }
                };

                let mut conn = Conn::new(stream, response, buf);
                if let Err(e) =
                    poll.registry().register(&mut conn.stream, tok, Interest::READABLE)
                {
                    eprintln!("[warn] register failed: {e}");
                    buf_pool.release(conn.read_buf);
                    token_pool.release(tok);
                    continue;
                }
                slab.insert(tok, conn);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => {
                eprintln!("[warn] accept error: {e}");
                break;
            }
        }
    }
}

fn handle_connection(
    token: Token,
    slab: &mut Slab,
    poll: &Poll,
    to_close: &mut Vec<Token>,
) {
    let Some(conn) = slab.get_mut(token) else { return };
    conn.touch();

    if !do_read(conn, token, to_close) {
        return;
    }

    if conn.request_complete() && !conn.has_pending_write() {
        conn.arm_write();
        let _ = poll.registry().reregister(
            &mut conn.stream,
            token,
            Interest::READABLE | Interest::WRITABLE,
        );
    }

    if conn.has_pending_write() {
        do_write(conn, token, poll, to_close);
    }
}

fn do_read(conn: &mut Conn, token: Token, to_close: &mut Vec<Token>) -> bool {
    loop {
        if conn.read_len >= BUF_SIZE {
            eprintln!("[warn] request too large, closing {:?}", token);
            to_close.push(token);
            return false;
        }
        let dst = &mut conn.read_buf[conn.read_len..];
        match conn.stream.read(dst) {
            Ok(0) => {
                to_close.push(token);
                return false;
            }
            Ok(n) => {
                conn.read_len += n;
                if conn.read_len > MAX_REQUEST_SIZE {
                    eprintln!("[warn] request too large, closing {:?}", token);
                    to_close.push(token);
                    return false;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return true,
            Err(e) => {
                eprintln!("[warn] read error on {:?}: {e}", token);
                to_close.push(token);
                return false;
            }
        }
    }
}

fn do_write(conn: &mut Conn, token: Token, poll: &Poll, to_close: &mut Vec<Token>) {
    loop {
        let slice = &conn.write_buf[conn.write_pos..];
        match conn.stream.write(slice) {
            Ok(n) => {
                conn.write_pos += n;
                if !conn.has_pending_write() {
                    let _ = poll
                        .registry()
                        .reregister(&mut conn.stream, token, Interest::READABLE);
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => {
                eprintln!("[warn] write error on {:?}: {e}", token);
                to_close.push(token);
                break;
            }
        }
    }
}

fn close_conn(
    slab: &mut Slab,
    token_pool: &mut TokenPool,
    buf_pool: &mut BufPool,
    poll: &Poll,
    tok: Token,
) {
    if let Some(mut c) = slab.slots[tok.0].take() {
        let _ = poll.registry().deregister(&mut c.stream);
        buf_pool.release(c.read_buf);
        token_pool.release(tok);
    }
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

    println!("Vrypt listening on {addr} ({cpus} threads)");

    let handles: Vec<_> = (0..cpus)
        .map(|_| thread::spawn(move || worker(addr, response)))
        .collect();

    for h in handles {
        if let Err(e) = h.join() {
            eprintln!("[error] thread panic: {e:?}");
        }
    }
}

