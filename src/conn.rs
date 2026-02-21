use crate::config::{BUF_SIZE, CONN_TIMEOUT};
use std::time::Instant;

pub struct Conn {
    pub stream: mio::net::TcpStream,
    pub read_buf: Box<[u8; BUF_SIZE]>,
    pub read_len: usize,
    pub write_buf: &'static [u8],
    pub write_pos: usize,
    pub last_active: Instant,
}

impl Conn {
    pub fn new(stream: mio::net::TcpStream, response: &'static [u8], buf: Box<[u8; BUF_SIZE]>) -> Self {
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
    pub fn filled(&self) -> &[u8] {
        &self.read_buf[..self.read_len]
    }

    #[inline]
    pub fn request_complete(&self) -> bool {
        self.filled().windows(4).any(|w| w == b"\r\n\r\n")
    }

    #[inline]
    pub fn has_pending_write(&self) -> bool {
        self.write_pos < self.write_buf.len()
    }

    #[inline]
    pub fn arm_write(&mut self) {
        self.read_len = 0;
        self.write_pos = 0;
    }

    #[inline]
    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    #[inline]
    pub fn is_timed_out(&self) -> bool {
        self.last_active.elapsed() > CONN_TIMEOUT
    }
}
