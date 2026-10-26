//! Trigora Pulse host.

mod authority;
mod drive;
mod error;
mod remote;
mod socket;
mod supervisor;

pub use authority::{
    AcceptedEvent, Authority, CancelReceipt, Claimed, Commit, CommitResult, CreatedChild,
    EventReceipt, InvokeIntent, InvokeReceipt, InvokeSpec, JournalRow, StartedChild, TimerRow,
    VersionPin, View, WaitRow,
};
pub use error::{Error, Result};
pub use remote::{select_remote, RemoteClient, RemoteMode};
pub use socket::{listen, Adapter, EffectRequest, Frame, Listener, Reply, ADAPTER_PROTOCOL};
pub use supervisor::{Config, Dispatch, Runtime, Span, Timings, VersionWait};

#[cfg(test)]
mod tests;
