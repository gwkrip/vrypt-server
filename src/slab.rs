use crate::conn::Conn;
use mio::Token;

pub struct Slab {
    pub slots: Vec<Option<Conn>>,
}

impl Slab {
    pub fn new(cap: usize) -> Self {
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(None);
        }
        Self { slots }
    }

    #[inline]
    pub fn insert(&mut self, tok: Token, conn: Conn) {
        self.slots[tok.0] = Some(conn);
    }

    #[inline]
    pub fn get_mut(&mut self, tok: Token) -> Option<&mut Conn> {
        self.slots[tok.0].as_mut()
    }

    pub fn iter_tokens(&self) -> impl Iterator<Item = Token> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|_| Token(i)))
    }
}
