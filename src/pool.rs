use crate::config::{BUF_SIZE, MAX_CONNS};
use mio::Token;

pub struct BufPool {
    free: Vec<Box<[u8; BUF_SIZE]>>,
    active: usize,
    max_active: usize,
    max_recycled: usize,
}

impl BufPool {
    pub fn new(max_active: usize, max_recycled: usize) -> Self {
        Self {
            free: Vec::with_capacity(max_recycled),
            active: 0,
            max_active,
            max_recycled,
        }
    }

    #[inline]
    pub fn acquire(&mut self) -> Option<Box<[u8; BUF_SIZE]>> {
        if self.active >= self.max_active {
            return None;
        }
        self.active += 1;
        Some(self.free.pop().unwrap_or_else(|| Box::new([0u8; BUF_SIZE])))
    }

    #[inline]
    pub fn release(&mut self, mut buf: Box<[u8; BUF_SIZE]>) {
        self.active = self.active.saturating_sub(1);
        if self.free.len() < self.max_recycled {
            buf.fill(0);
            self.free.push(buf);
        }
    }
}

pub struct TokenPool {
    next: usize,
    free: Vec<usize>,
}

impl TokenPool {
    pub fn new() -> Self {
        Self { next: 1, free: Vec::with_capacity(MAX_CONNS) }
    }

    #[inline]
    pub fn acquire(&mut self) -> Option<Token> {
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
    pub fn release(&mut self, tok: Token) {
        self.free.push(tok.0);
    }
}
