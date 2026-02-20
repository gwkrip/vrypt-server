use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::os::unix::io::IntoRawFd;
use std::thread;

const RESPONSE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\nVrypt";

const SERVER: Token = Token(0);
const DEFAULT_PORT: u16 = 8080;

fn worker(addr: SocketAddr) {
    let sock = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    sock.set_reuse_address(true).unwrap();
    sock.set_reuse_port(true).unwrap();
    sock.set_nonblocking(true).unwrap();
    sock.bind(&addr.into()).unwrap();
    sock.listen(4096).unwrap();

    let std_listener = unsafe { std::net::TcpListener::from_raw_fd(sock.into_raw_fd()) };
    let mut listener = TcpListener::from_std(std_listener);

    let mut poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1024);
    let mut conns: HashMap<Token, mio::net::TcpStream> = HashMap::new();
    let mut next_token: usize = 1;
    let mut buf = [0u8; 4096];

    poll.registry()
        .register(&mut listener, SERVER, Interest::READABLE)
        .unwrap();

    loop {
        poll.poll(&mut events, None).unwrap();

        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_nodelay(true);
                            let tok = Token(next_token);
                            next_token = next_token.wrapping_add(1).max(1);
                            poll.registry()
                                .register(&mut stream, tok, Interest::READABLE)
                                .unwrap();
                            conns.insert(tok, stream);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                },
                token => {
                    let mut dead = false;
                    if let Some(stream) = conns.get_mut(&token) {
                        loop {
                            match stream.read(&mut buf) {
                                Ok(0) => {
                                    dead = true;
                                    break;
                                }
                                Ok(_) => {
                                    let mut sent = 0;
                                    while sent < RESPONSE.len() {
                                        match stream.write(&RESPONSE[sent..]) {
                                            Ok(n) => sent += n,
                                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                                            Err(_) => {
                                                dead = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                                Err(_) => {
                                    dead = true;
                                    break;
                                }
                            }
                            if dead {
                                break;
                            }
                        }
                    }
                    if dead {
                        if let Some(mut s) = conns.remove(&token) {
                            let _ = poll.registry().deregister(&mut s);
                        }
                    }
                }
            }
        }
    }
}

fn parse_args() -> SocketAddr {
    let mut args = std::env::args().skip(1);
    let port = match args.next().as_deref() {
        Some("-p") | Some("--port") => args
            .next()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or_else(|| {
                eprintln!("Invalid port, using default {}", DEFAULT_PORT);
                DEFAULT_PORT
            }),
        Some(v) => v.parse::<u16>().unwrap_or_else(|_| {
            eprintln!("Invalid port '{}', using default {}", v, DEFAULT_PORT);
            DEFAULT_PORT
        }),
        None => DEFAULT_PORT,
    };

    format!("0.0.0.0:{}", port).parse().unwrap()
}

fn main() {
    let addr = parse_args();
    let cpus = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);

    println!("Vrypt listening on {} ({} threads)", addr, cpus);

    let handles: Vec<_> = (0..cpus)
        .map(|_| thread::spawn(move || worker(addr)))
        .collect();

    for h in handles {
        let _ = h.join();
    }
}
