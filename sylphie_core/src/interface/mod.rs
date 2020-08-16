//! Handles logging, terminal input, error reporting and related concerns.

use arc_swap::ArcSwapOption;
use crate::errors::*;
use crate::global_instance::InstanceScopeGuard;
use crate::module::CrateMetadata;
use parking_lot::Mutex;
use static_events::prelude_async::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod error_report;
mod logger;
mod terminal;

pub use logger::SetupLoggerEvent;
pub use terminal::TerminalCommandEvent;

// TODO: Replace with BotInfo
pub(crate) struct InterfaceInfo {
    pub bot_name: String,
    pub root_path: PathBuf,
}

struct InterfaceShared {
    info: InterfaceInfo,
    is_shutdown: AtomicBool,
    loaded_crates: ArcSwapOption<Box<[CrateMetadata]>>,
}

struct InterfaceData {
    shared: Arc<InterfaceShared>,
    terminal: Arc<terminal::Terminal>,
    current_logger: Arc<Mutex<Option<logger::Logger>>>,
    scope_guard: InstanceScopeGuard<error_report::ErrorCtx>,
}
struct LoggerLockGuard<'a>(&'a InterfaceData);
impl <'a> Drop for LoggerLockGuard<'a> {
    fn drop(&mut self) {
        *self.0.current_logger.lock() = None;
    }
}

/// A handle to services related to logging, the user interface, and error reporting.
#[derive(Clone)]
pub struct Interface(Arc<InterfaceData>);
impl Interface {
    pub(crate) fn new(info: InterfaceInfo) -> Result<Interface> {
        let shared = Arc::new(InterfaceShared {
            info,
            is_shutdown: AtomicBool::new(false),
            loaded_crates: ArcSwapOption::empty(),
        });
        let error_ctx = error_report::ErrorCtx::new(shared.clone()).activate();
        let terminal = Arc::new(terminal::Terminal::new(shared.clone())?);
        Ok(Interface(Arc::new(InterfaceData {
            shared,
            terminal,
            current_logger: Arc::new(Mutex::new(None)),
            scope_guard: error_ctx,
        })))
    }

    pub(crate) fn start(&self, target: &Handler<impl Events>) -> Result<()> {
        let _lock_guard = {
            let mut lock = self.0.current_logger.lock();
            let logger = logger::activate(target, self.0.shared.clone(), self.0.terminal.clone())?;
            *lock = Some(logger);
            LoggerLockGuard(&self.0)
        };
        self.0.terminal.start_terminal(target)?;
        Ok(())
    }

    pub(crate) fn shutdown(&self) {
        self.0.shared.is_shutdown.store(true, Ordering::Relaxed)
    }

    pub(crate) fn set_loaded_crates(&self, crates: Arc<[CrateMetadata]>) {
        self.0.shared.loaded_crates.store(Some(Arc::new(crates.to_vec().into())));
    }

    /// Reloads the logger, to reflect any configuration changes that may have occurred since.
    ///
    /// If no logger is currently active, this method will return an error.
    pub fn reload_logger(&self, target: &Handler<impl Events>) -> Result<()> {
        let mut lock = self.0.current_logger.lock();
        let handle = lock.as_mut().internal_err(|| "Logger is not running.")?;
        logger::reload(target, handle)
    }
}

impl Error {
    /// Reports this error to the user.
    pub fn report_error(&self) {
        error_report::report_error(self);
    }
}

pub(crate) fn init_interface() {
    logger::activate_log_compat();
    logger::activate_fallback();
    error_report::init_deadlock_detection();
}
pub(crate) fn get_info_string() -> String {
    error_report::get_info_string()
}