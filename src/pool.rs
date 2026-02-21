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
    pub fn release(&mut self, buf: Box<[u8; BUF_SIZE]>) {
        if self.active == 0 {
            eprintln!("[bug] BufPool::release called with active == 0 (double-release?)");
            return;
        }
        self.active -= 1;
        if self.free.len() < self.max_recycled {
            self.free.push(buf);
        }
    }
}

pub struct TokenPool {
    next: usize,
    free: Vec<usize>,
    in_use: Vec<bool>,
}

impl TokenPool {
    pub fn new() -> Self {
        Self {
            next: 1,
            free: Vec::with_capacity(MAX_CONNS),
            in_use: vec![false; MAX_CONNS],
        }
    }

    #[inline]
    pub fn acquire(&mut self) -> Option<Token> {
        if let Some(t) = self.free.pop() {
            debug_assert!(!self.in_use[t], "TokenPool: acquired a token that was marked in-use");
            self.in_use[t] = true;
            return Some(Token(t));
        }
        if self.next < MAX_CONNS {
            let t = self.next;
            self.next += 1;
            self.in_use[t] = true;
            Some(Token(t))
        } else {
            None
        }
    }

    #[inline]
    pub fn release(&mut self, tok: Token) {
        let t = tok.0;
        if t == 0 || t >= MAX_CONNS {
            eprintln!("[bug] TokenPool::release: token {t} out of valid range");
            return;
        }
        if !self.in_use[t] {
            eprintln!("[bug] TokenPool::release: token {t} double-released");
            return;
        }
        self.in_use[t] = false;
        self.free.push(t);
    }
}
