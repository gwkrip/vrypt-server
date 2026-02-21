use crate::config::{BUF_SIZE, CONN_TIMEOUT, MAX_CONNS, MAX_RECYCLED_BUFS, MAX_REQUEST_SIZE, POLL_TIMEOUT, SERVER_TOKEN};
use crate::conn::Conn;
use crate::counter::RpsCounter;
use crate::pool::{BufPool, TokenPool};
use crate::slab::Slab;
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use socket2::{Domain, Protocol, Socket, Type};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::time::Instant;

#[derive(Eq, PartialEq)]
struct TimeoutEntry {
    deadline: Reverse<Instant>,
    token: Token,
    generation: u64,
}

impl Ord for TimeoutEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.deadline.cmp(&other.deadline)
    }
}

impl PartialOrd for TimeoutEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub fn worker(addr: SocketAddr, response: &'static [u8], counter: &'static RpsCounter, thread_id: usize) {
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
    let mut to_close: Vec<Token> = Vec::with_capacity(64);
    let mut timeout_heap: BinaryHeap<TimeoutEntry> = BinaryHeap::with_capacity(MAX_CONNS);

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
        let now = Instant::now();

        loop {
            match timeout_heap.peek() {
                Some(entry) if entry.deadline.0 <= now => {
                    let entry = timeout_heap.pop().unwrap();
                    let tok = entry.token;
                    match slab.get(tok) {
                        Some(conn) if conn.generation == entry.generation => {
                            eprintln!("[info] timeout, closing {:?}", tok);
                            to_close.push(tok);
                        }
                        _ => {}
                    }
                }
                _ => break,
            }
        }

        for event in events.iter() {
            match event.token() {
                SERVER_TOKEN => {
                    accept_connections(
                        &mut listener, &mut slab, &mut token_pool,
                        &mut buf_pool, &poll, response, &mut timeout_heap,
                    );
                }
                token => {
                    handle_connection(token, &mut slab, &poll, &mut to_close, counter, thread_id, &mut timeout_heap);
                }
            }
        }

        for tok in to_close.drain(..) {
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
    timeout_heap: &mut BinaryHeap<TimeoutEntry>,
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

                if let Err(e) = poll.registry().register(&mut conn.stream, tok, Interest::READABLE) {
                    eprintln!("[warn] register failed: {e}");
                    buf_pool.release(conn.read_buf);
                    token_pool.release(tok);
                    continue;
                }

                let deadline = conn.last_active + CONN_TIMEOUT;
                let generation = conn.generation;
                slab.insert(tok, conn);

                timeout_heap.push(TimeoutEntry {
                    deadline: Reverse(deadline),
                    token: tok,
                    generation,
                });
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
    counter: &'static RpsCounter,
    thread_id: usize,
    timeout_heap: &mut BinaryHeap<TimeoutEntry>,
) {
    let Some(conn) = slab.get_mut(token) else { return };

    conn.touch();
    let new_deadline = conn.last_active + CONN_TIMEOUT;
    let new_generation = conn.generation;
    timeout_heap.push(TimeoutEntry {
        deadline: Reverse(new_deadline),
        token,
        generation: new_generation,
    });

    if !conn.has_pending_write() {
        if !do_read(conn, token, to_close) {
            return;
        }

        if conn.request_complete() {
            conn.arm_write();
            let _ = poll.registry().reregister(
                &mut conn.stream, token,
                Interest::READABLE | Interest::WRITABLE,
            );
        }
    }

    if conn.has_pending_write() {
        do_write(conn, token, poll, to_close, counter, thread_id);
    }
}

fn do_read(conn: &mut Conn, token: Token, to_close: &mut Vec<Token>) -> bool {
    loop {
        if conn.read_len >= BUF_SIZE {
            eprintln!("[warn] buffer full, closing {:?}", token);
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
                    eprintln!("[warn] request too large (>{} bytes), closing {:?}", MAX_REQUEST_SIZE, token);
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

fn do_write(
    conn: &mut Conn,
    token: Token,
    poll: &Poll,
    to_close: &mut Vec<Token>,
    counter: &'static RpsCounter,
    thread_id: usize,
) {
    let mut current_pos = match conn.write_pos {
        Some(p) => p,
        None => return,
    };

    loop {
        let slice = &conn.write_buf[current_pos..];
        match conn.stream.write(slice) {
            Ok(n) => {
                current_pos += n;
                conn.write_pos = Some(current_pos);
                if !conn.has_pending_write() {
                    counter.increment(thread_id);
                    conn.reset_for_read();
                    let _ = poll.registry().reregister(
                        &mut conn.stream, token,
                        Interest::READABLE,
                    );
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
    if let Some(mut c) = slab.remove(tok) {
        let _ = poll.registry().deregister(&mut c.stream);
        buf_pool.release(c.read_buf);
        token_pool.release(tok);
    }
}
