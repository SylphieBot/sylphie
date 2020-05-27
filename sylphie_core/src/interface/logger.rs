use chrono::Local;
use crate::errors::*;
use crate::interface::InterfaceShared;
use crate::interface::terminal::Terminal;
use parking_lot::{Mutex, const_mutex};
use static_events::*;
use std::fmt::{Result as FmtResult, Write};
use std::path::PathBuf;
use std::sync::{Arc, Once};
use tracing::{*, Metadata, Event};
use tracing::span::{Attributes, Record};
use tracing::subscriber::{DefaultGuard, Interest};
use tracing_subscriber::{FmtSubscriber, EnvFilter, Layer};
use tracing_subscriber::fmt::format::{DefaultFields, Format, Full};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::{Context, Layered};

// TODO: Logging to file.

type EnvSubscriber = FmtSubscriber<DefaultFields, Format<Full, ShortFormatTime>, EnvFilter>;

struct LockingSubscriber {
    terminal: Arc<Terminal>,
    underlying: EnvSubscriber,
}
impl Subscriber for LockingSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.underlying.enabled(metadata)
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        self.underlying.new_span(span)
    }
    fn record(&self, span: &Id, values: &Record<'_>) {
        self.underlying.record(span, values)
    }
    fn record_follows_from(&self, span: &Id, follows: &Id) {
        self.underlying.record_follows_from(span, follows)
    }
    fn enter(&self, span: &Id) {
        self.underlying.enter(span)
    }
    fn exit(&self, span: &Id) {
        self.underlying.exit(span)
    }
    fn event(&self, event: &Event<'_>) {
        let _guard = self.terminal.lock_write();
        self.underlying.event(event);
    }
}

struct ShortFormatTime;
impl FormatTime for ShortFormatTime {
    fn format_time(&self, w: &mut dyn Write) -> FmtResult {
        write!(w, "{}", Local::now().format("[%k:%M:%S]"))
    }
}

pub struct Logger {
    guard: Option<DefaultGuard>,
    shared: Arc<InterfaceShared>,
    terminal: Arc<Terminal>,
}

pub fn activate_log_compat() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        tracing_log::LogTracer::init().expect("Could not set log compat logger.");
    });
}

/// An event that is sent by the logging framework to configure logging.
pub struct SetupLoggerEvent {
    pub console: tracing_subscriber::EnvFilter,
}
self_event!(SetupLoggerEvent);

pub fn activate_fallback() {
    static ONCE: Once = Once::new();
    static LOGGER_MUTEX: Mutex<Option<DefaultGuard>> = const_mutex(None);

    ONCE.call_once(|| {
        let env_filter = tracing_subscriber::EnvFilter::new("debug");
        let subscriber = tracing_subscriber::FmtSubscriber::builder()
            .with_timer(ShortFormatTime)
            .with_env_filter(env_filter)
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        *LOGGER_MUTEX.lock() = Some(guard);
    });
}

fn log_path(shared: &InterfaceShared) -> Result<PathBuf> {
    let mut log_path = shared.info.root_path.clone();
    log_path.push("logs");

    if !log_path.exists() {
        std::fs::create_dir_all(&log_path)?;
    }
    ensure!(log_path.is_dir(), "Log directory is not a directory.");

    Ok(log_path)
}
fn make_logger(
    core: &Handler<impl Events>, shared: &InterfaceShared, terminal: &Arc<Terminal>,
) -> Result<LockingSubscriber> {
    let log_path = log_path(shared)?;

    let ev = core.dispatch(SetupLoggerEvent {
        console: tracing_subscriber::EnvFilter::new("info"),
    });

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_timer(ShortFormatTime)
        .with_env_filter(ev.console)
        .finish();
    Ok(LockingSubscriber {
        terminal: terminal.clone(),
        underlying: subscriber,
    })
}
pub(in super) fn activate(
    core: &Handler<impl Events>, shared: Arc<InterfaceShared>, terminal: Arc<Terminal>,
) -> Result<Logger> {
    activate_log_compat();
    let new_logger = make_logger(core, &shared, &terminal)?;
    let guard = tracing::subscriber::set_default(new_logger);
    Ok(Logger { guard: Some(guard), shared, terminal })
}
pub fn reload(
    core: &Handler<impl Events>, guard: &mut Logger,
) -> Result<()> {
    activate_log_compat(); // More a procaution than anything
    let new_logger = make_logger(core, &guard.shared, &guard.terminal)?;
    guard.guard = None; // Drop the old guard first. The fallback will take over for a bit.
    guard.guard = Some(tracing::subscriber::set_default(new_logger)); // Set the new logger.
    Ok(())
}