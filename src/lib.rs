//! openHLSW protocol core: server addresses, Valve A2S, GameSpy 2 and the other query
//! protocols of HLSW's game list (`query`), RCON.
//! Everything here is blocking std networking with timeouts; the GUI runs it on
//! worker threads.

pub mod a2s;
pub mod address;
pub mod gamespy2;
pub mod query;
pub mod rcon;
pub mod stats;
pub mod steamid;
