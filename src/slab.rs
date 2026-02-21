use crate::conn::Conn;
use mio::Token;

pub struct Slab {
    slots: Vec<Option<Box<Conn>>>,
}

impl Slab {
    pub fn new(cap: usize) -> Self {
        Self {
            slots: (0..cap).map(|_| None).collect(),
        }
    }

    #[inline]
    pub fn insert(&mut self, tok: Token, conn: Conn) {
        self.slots[tok.0] = Some(Box::new(conn));
    }

    #[inline]
    pub fn get(&self, tok: Token) -> Option<&Conn> {
        self.slots[tok.0].as_deref()
    }

    #[inline]
    pub fn get_mut(&mut self, tok: Token) -> Option<&mut Conn> {
        self.slots[tok.0].as_deref_mut()
    }

    #[inline]
    pub fn remove(&mut self, tok: Token) -> Option<Conn> {
        self.slots[tok.0].take().map(|b| *b)
    }
}
