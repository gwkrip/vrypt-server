use crate::config::{STATS_INTERVAL, STATS_METRIC, STATS_TARGET};
use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

#[repr(align(64))]
pub struct Slot {
    pub count: AtomicU64,
    _pad: [u8; 56],
}

pub struct RpsCounter {
    slots: Box<[Slot]>,
}

impl RpsCounter {
    pub fn new(num_threads: usize) -> &'static Self {
        let slots = (0..num_threads)
            .map(|_| Slot { count: AtomicU64::new(0), _pad: [0u8; 56] })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Box::leak(Box::new(Self { slots }))
    }

    #[inline]
    pub fn increment(&self, thread_id: usize) {
        self.slots[thread_id].count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn total(&self) -> u64 {
        self.slots.iter().map(|s| s.count.load(Ordering::Relaxed)).sum()
    }
}

pub fn spawn_stats_pusher(counter: &'static RpsCounter) {
    thread::spawn(move || {
        let sock = match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[stats] failed to bind udp socket: {e}");
                return;
            }
        };

        let target: SocketAddr = match STATS_TARGET.parse() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[stats] invalid target address: {e}");
                return;
            }
        };

        let mut buf = [0u8; 64];
        let mut prev: u64 = 0;

        loop {
            thread::sleep(STATS_INTERVAL);

            let total = counter.total();
            let rps = total.wrapping_sub(prev);
            prev = total;

            let mut cursor = std::io::Cursor::new(&mut buf[..]);
            if write!(cursor, "{}:{}|g", STATS_METRIC, rps).is_err() {
                eprintln!(
                    "[stats] message too long for buffer (metric='{}', rps={rps}); skipping",
                    STATS_METRIC
                );
                continue;
            }
            let n = cursor.position() as usize;
            let _ = sock.send_to(&buf[..n], target);
        }
    });
}
