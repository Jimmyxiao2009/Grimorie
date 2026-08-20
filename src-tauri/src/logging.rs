//! Application logging.
//!
//! Logs go to a rolling file inside the app's log directory, and to stderr in
//! debug builds. What is deliberately *not* logged matters as much as what is:
//! no API keys, no manuscript text. Call sites are responsible for honouring
//! that.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

/// Initialises tracing. The returned guard must be kept alive for the lifetime
/// of the process, otherwise buffered log lines are dropped on exit.
pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("GRIMOIRE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("grimoire=info,warn"));

    let (writer, guard) = match std::fs::create_dir_all(log_dir) {
        Ok(()) => {
            let appender = tracing_appender::rolling::daily(log_dir, "grimoire.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(writer), Some(guard))
        }
        Err(err) => {
            eprintln!("grimoire: could not open log directory {log_dir:?}: {err}");
            (None, None)
        }
    };

    let registry = tracing_subscriber::registry().with(filter);

    let file_layer = writer.map(|writer| fmt::layer().with_ansi(false).with_writer(writer));

    #[cfg(debug_assertions)]
    let registry = registry.with(fmt::layer().with_writer(std::io::stderr));

    registry.with(file_layer).init();

    guard
}
