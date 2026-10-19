//! openHLSW protocol core: server addresses, Valve A2S and GameSpy 2 queries, RCON.
//! Everything here is blocking std networking with timeouts; the GUI runs it on
//! worker threads.

pub mod a2s;
pub mod address;
pub mod gamespy2;
pub mod rcon;
pub mod stats;
