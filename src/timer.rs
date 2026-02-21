use mio::Token;
use std::time::{Duration, Instant};

const WHEEL_SIZE: usize = 64;
const WHEEL_MASK: usize = WHEEL_SIZE - 1;
const SLOT_DURATION: Duration = Duration::from_secs(1);

pub struct TimerWheel {
    slots: Vec<Vec<(Token, u64)>>,
    cursor: usize,
    last_tick: Instant,
    timeout_slots: usize,
}

impl TimerWheel {
    pub fn new(timeout: Duration) -> Self {
        let timeout_slots = (timeout.as_secs() as usize + 1).min(WHEEL_MASK);
        Self {
            slots: vec![Vec::new(); WHEEL_SIZE],
            cursor: 0,
            last_tick: Instant::now(),
            timeout_slots,
        }
    }

    #[inline]
    pub fn add(&mut self, token: Token, generation: u64) {
        let slot = (self.cursor + self.timeout_slots) & WHEEL_MASK;
        self.slots[slot].push((token, generation));
    }

    pub fn advance(&mut self, now: Instant, out: &mut Vec<(Token, u64)>) {
        let elapsed_ms = now.duration_since(self.last_tick).as_millis();
        let ticks = ((elapsed_ms / 1_000) as usize).min(WHEEL_SIZE);
        if ticks == 0 {
            return;
        }
        for _ in 0..ticks {
            self.cursor = (self.cursor + 1) & WHEEL_MASK;
            out.extend(self.slots[self.cursor].drain(..));
        }
        self.last_tick += SLOT_DURATION * ticks as u32;
    }
}
