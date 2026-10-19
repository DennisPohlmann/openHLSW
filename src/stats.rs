//! Global network traffic counters (shown in the status bar like HLSW does).

use std::sync::atomic::{AtomicU64, Ordering};

static SENT: AtomicU64 = AtomicU64::new(0);
static RECEIVED: AtomicU64 = AtomicU64::new(0);

pub fn add_sent(bytes: usize) {
    SENT.fetch_add(bytes as u64, Ordering::Relaxed);
}

pub fn add_received(bytes: usize) {
    RECEIVED.fetch_add(bytes as u64, Ordering::Relaxed);
}

/// Total bytes (sent, received) since program start.
pub fn totals() -> (u64, u64) {
    (
        SENT.load(Ordering::Relaxed),
        RECEIVED.load(Ordering::Relaxed),
    )
}
