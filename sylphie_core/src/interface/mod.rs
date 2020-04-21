//! Handles logging, terminal input, error reporting and related concerns.

use crate::errors::*;
use crate::module::CrateMetadata;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod logger;
mod terminal;

pub use terminal::TerminalCommandEvent;

pub struct InterfaceInfo {
    pub bot_name: String,
    pub root_path: PathBuf,
    pub loaded_crates: Vec<CrateMetadata>,
}

struct InterfaceShared {
    info: InterfaceInfo,
    is_shutdown: AtomicBool,
}

struct InterfaceData {
    shared: Arc<InterfaceShared>,
    terminal: Arc<terminal::Terminal>,
}

#[derive(Clone)]
pub struct Interface(Arc<InterfaceData>);
impl Interface {
    pub fn new(info: InterfaceInfo) -> Result<Interface> {
        let shared = Arc::new(InterfaceShared {
            info,
            is_shutdown: AtomicBool::new(false),
        });
        let terminal = Arc::new(terminal::Terminal::new(shared.clone())?);
        Ok(Interface(Arc::new(InterfaceData {
            shared, terminal,
        })))
    }

    pub fn shutdown(&self) {
        self.0.shared.is_shutdown.store(true, Ordering::Relaxed)
    }
}