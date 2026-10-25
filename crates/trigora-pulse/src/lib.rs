//! Trigora Pulse host.

mod authority;
mod drive;
mod error;
mod remote;
mod socket;
mod supervisor;

pub use authority::{
    Authority, Claimed, Commit, CommitResult, JournalRow, TimerRow, VersionPin, View, WaitRow,
};
pub use error::{Error, Result};
pub use remote::{run_remote_measure, RemoteClient, RemoteReport};
pub use socket::{listen, Adapter, EffectRequest, Frame, Listener, Reply};
pub use supervisor::{Config, Dispatch, Runtime, Span, Timings, VersionWait};

#[cfg(test)]
mod tests;
