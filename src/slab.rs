use crate::conn::Conn;
use mio::Token;

pub struct Slab {
    slots: Vec<Option<Conn>>,
}

impl Slab {
    pub fn new(cap: usize) -> Self {
        Self { slots: vec![None; cap] }
    }

    #[inline]
    pub fn insert(&mut self, tok: Token, conn: Conn) {
        self.slots[tok.0] = Some(conn);
    }

    #[inline]
    pub fn get(&self, tok: Token) -> Option<&Conn> {
        self.slots[tok.0].as_ref()
    }

    #[inline]
    pub fn get_mut(&mut self, tok: Token) -> Option<&mut Conn> {
        self.slots[tok.0].as_mut()
    }

    #[inline]
    pub fn remove(&mut self, tok: Token) -> Option<Conn> {
        self.slots[tok.0].take()
    }
}
