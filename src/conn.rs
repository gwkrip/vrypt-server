use crate::config::BUF_SIZE;
use std::time::Instant;

pub struct Conn {
    pub stream: mio::net::TcpStream,
    pub read_buf: Box<[u8; BUF_SIZE]>,
    pub read_len: usize,
    pub scan_offset: usize,
    pub write_buf: &'static [u8],
    pub write_pos: Option<usize>,
    pub last_active: Instant,
}

impl Conn {
    pub fn new(stream: mio::net::TcpStream, response: &'static [u8], buf: Box<[u8; BUF_SIZE]>) -> Self {
        Self {
            stream,
            read_buf: buf,
            read_len: 0,
            scan_offset: 0,
            write_buf: response,
            write_pos: None,
            last_active: Instant::now(),
        }
    }

    #[inline]
    pub fn request_complete(&mut self) -> bool {
        let start = self.scan_offset.saturating_sub(3);
        let found = self.read_buf[start..self.read_len]
            .windows(4)
            .any(|w| w == b"\r\n\r\n");
        if self.read_len >= 3 {
            self.scan_offset = self.read_len - 3;
        }
        found
    }

    #[inline]
    pub fn has_pending_write(&self) -> bool {
        matches!(self.write_pos, Some(pos) if pos < self.write_buf.len())
    }

    #[inline]
    pub fn arm_write(&mut self) {
        self.read_len = 0;
        self.scan_offset = 0;
        self.write_pos = Some(0);
    }

    #[inline]
    pub fn reset_for_read(&mut self) {
        self.write_pos = None;
    }

    #[inline]
    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }
}
