mod error;
mod http;
mod product;
mod runtime;
mod value;

pub use error::LocalError;
pub use http::Listeners;
pub use runtime::{EffectEndpoint, EventSink, FileProgram, LocalRuntime};

use chrono::{SecondsFormat, Utc};

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests;
