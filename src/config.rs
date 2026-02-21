use mio::Token;
use std::time::Duration;

pub const SERVER_TOKEN: Token = Token(0);
pub const DEFAULT_PORT: u16 = 8080;
pub const CONN_TIMEOUT: Duration = Duration::from_secs(30);
pub const POLL_TIMEOUT: Duration = Duration::from_millis(5000);
pub const MAX_REQUEST_SIZE: usize = 64 * 1024;
pub const RESPONSE_BODY: &[u8] = b"Vrypt";
pub const MAX_CONNS: usize = 65536;
pub const BUF_SIZE: usize = 64 * 1024;
pub const MAX_RECYCLED_BUFS: usize = 256;
pub const STATS_INTERVAL: Duration = Duration::from_secs(1);
pub const STATS_TARGET: &str = "127.0.0.1:8125";
pub const STATS_METRIC: &str = "vrypt.rps";
