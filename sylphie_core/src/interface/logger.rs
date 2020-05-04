use chrono::Local;
use crate::errors::*;
use crate::interface::InterfaceShared;
use crate::interface::terminal::Terminal;
use parking_lot::Mutex;
use std::fmt::{Result as FmtResult, Write};
use std::sync::{Arc, Once};
use tracing::*;
use tracing::span::{Attributes, Record};
use tracing::subscriber::DefaultGuard;
use tracing_subscriber::{FmtSubscriber, EnvFilter};
use tracing_subscriber::fmt::format::{DefaultFields, Format, Full};
use tracing_subscriber::fmt::time::FormatTime;

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

pub struct ShortFormatTime;
impl FormatTime for ShortFormatTime {
    fn format_time(&self, w: &mut dyn Write) -> FmtResult {
        write!(w, "{}", Local::now().format("[%k:%M:%S]"))
    }
}

pub struct Logger {
    _guard: Mutex<Option<DefaultGuard>>,
}

fn activate_log_compat() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        tracing_log::LogTracer::init().expect("Could not set log compat logger.");
    });
}

pub(in super) fn activate(
    shared: Arc<InterfaceShared>, terminal: Arc<Terminal>,
) -> Result<Logger> {
    let mut log_path = shared.info.root_path.clone();
    log_path.push("logs");

    activate_log_compat();

    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_timer(ShortFormatTime)
        .with_env_filter(env_filter)
        .finish();
    let subscriber = LockingSubscriber {
        terminal: terminal.clone(),
        underlying: subscriber,
    };
    let guard = tracing::subscriber::set_default(subscriber);

    std::fs::create_dir_all(&shared.info.root_path)?;

    Ok(Logger { _guard: Mutex::new(Some(guard)) })
}

