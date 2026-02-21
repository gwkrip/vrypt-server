use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
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

fn build_response(body: &[u8]) -> Vec<u8> {
    let mut res = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    )
    .into_bytes();
    res.extend_from_slice(body);
    res
}

struct TokenPool {
    next: usize,
    free: Vec<usize>,
}

impl TokenPool {
    fn new() -> Self {
        Self { next: 1, free: Vec::new() }
    }

    fn acquire(&mut self) -> Token {
        match self.free.pop() {
            Some(t) => Token(t),
            None => {
                let t = self.next;
                self.next = self.next.checked_add(1).expect("token pool exhausted");
                Token(t)
            }
        }
    }

    fn release(&mut self, tok: Token) {
        self.free.push(tok.0);
    }
}

struct Conn {
    stream: mio::net::TcpStream,
    read_buf: Vec<u8>,
    write_buf: &'static [u8],
    write_pos: usize,
    last_active: Instant,
}

impl Conn {
    fn new(stream: mio::net::TcpStream, response: &'static [u8]) -> Self {
        Self {
            stream,
            read_buf: Vec::with_capacity(1024),
            write_buf: response,
            write_pos: usize::MAX,
            last_active: Instant::now(),
        }
    }

    fn request_complete(&self) -> bool {
        self.read_buf.windows(4).any(|w| w == b"\r\n\r\n")
    }

    fn has_pending_write(&self) -> bool {
        self.write_pos < self.write_buf.len()
    }

    fn arm_write(&mut self) {
        self.read_buf.clear();
        self.write_pos = 0;
    }

    fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    fn is_timed_out(&self) -> bool {
        self.last_active.elapsed() > CONN_TIMEOUT
    }
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
    let mut conns: HashMap<Token, Conn> = HashMap::new();
    let mut pool = TokenPool::new();

    let mut to_close: Vec<Token> = Vec::new();
    let mut timed_out: Vec<Token> = Vec::new();

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
                    accept_connections(&mut listener, &mut conns, &mut pool, &poll, response)
                }
                token => {
                    handle_connection(token, &mut conns, &poll, &mut to_close)
                }
            }
        }

        for tok in to_close.drain(..) {
            close_conn(&mut conns, &mut pool, &poll, tok);
        }

        timed_out.clear();
        conns
            .iter()
            .filter(|(_, c)| c.is_timed_out())
            .for_each(|(t, _)| timed_out.push(*t));

        for tok in timed_out.drain(..) {
            eprintln!("[info] timeout, closing {:?}", tok);
            close_conn(&mut conns, &mut pool, &poll, tok);
        }
    }
}

fn accept_connections(
    listener: &mut TcpListener,
    conns: &mut HashMap<Token, Conn>,
    pool: &mut TokenPool,
    poll: &Poll,
    response: &'static [u8],
) {
    loop {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let _ = stream.set_nodelay(true);
                let tok = pool.acquire();
                let mut conn = Conn::new(stream, response);
                if let Err(e) =
                    poll.registry().register(&mut conn.stream, tok, Interest::READABLE)
                {
                    eprintln!("[warn] register failed: {e}");
                    pool.release(tok);
                    continue;
                }
                conns.insert(tok, conn);
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
    conns: &mut HashMap<Token, Conn>,
    poll: &Poll,
    to_close: &mut Vec<Token>,
) {
    let Some(conn) = conns.get_mut(&token) else { return };
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
    let mut tmp = [0u8; 4096];
    loop {
        match conn.stream.read(&mut tmp) {
            Ok(0) => {
                to_close.push(token);
                return false;
            }
            Ok(n) => {
                conn.read_buf.extend_from_slice(&tmp[..n]);
                if conn.read_buf.len() > MAX_REQUEST_SIZE {
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

fn close_conn(conns: &mut HashMap<Token, Conn>, pool: &mut TokenPool, poll: &Poll, tok: Token) {
    if let Some(mut c) = conns.remove(&tok) {
        let _ = poll.registry().deregister(&mut c.stream);
        pool.release(tok);
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
