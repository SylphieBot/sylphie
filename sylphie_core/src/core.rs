use crate::errors::*;
use crate::interface::*;
use crate::module::{Module, ModuleManager};
use fs2::*;
use static_events::*;
use std::env;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::abort;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// TODO: Introduce builder for SylphieCore.
// TODO: Add lock/database support.

fn check_lock(path: impl AsRef<Path>) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    let lock_file = options.open(path)?;
    lock_file.try_lock_exclusive()?;
    Ok(lock_file)
}
fn get_exe_dir() -> PathBuf {
    let mut path = env::current_exe().expect("cannot get current exe path");
    path.pop();
    path
}
fn get_root_path() -> PathBuf {
    match env::var_os("CARGO_MANIFEST_DIR") {
        Some(manifest_dir) => PathBuf::from(manifest_dir),
        None => get_exe_dir(),
    }
}

/// The [`Events`] implementation used for a particular [`SylphieCore`].
#[derive(Events)]
pub struct SylphieEvents<R: Module> {
    #[subhandler] root_module: R,
    #[service] module_manager: ModuleManager,
    #[service] interface: Interface,
}

#[events_impl]
impl <R: Module> SylphieEvents<R> {
    #[event_handler]
    fn inherent_commands(&self, command: &TerminalCommandEvent) {
        match command.0.as_str().trim() {
            ".help" => {
                info!("Internal commands:");
                info!(" .help - Shows this help message");
                info!(" .shutdown - Forcefully shuts down the bot");
            }
            ".shutdown" => {
                info!("(shutdown)");
                ::std::process::abort()
            }
            _ => { }
        }
    }
}

struct CoreData<R: Module> {
    is_started: AtomicBool,
    bot_name: String,
    root_path: PathBuf,
    events: EventsHandle<SylphieEvents<R>>,
}

pub struct SylphieCore<R: Module>(Arc<CoreData<R>>);
impl <R: Module> SylphieCore<R> {
    pub fn new(bot_name: impl Into<String>) -> Self {
        SylphieCore(Arc::new(CoreData {
            is_started: AtomicBool::new(false),
            bot_name: bot_name.into(),
            root_path: get_root_path(),
            events: EventsHandle::new(),
        }))
    }

    /// Starts the bot core, blocking the main thread until the bot returns.
    pub fn start(&self) -> Result<()> {
        if !self.0.is_started.compare_and_swap(false, true, Ordering::Relaxed) {
            let (module_manager, root_module) = ModuleManager::init(self.clone());
            let loaded_crates = module_manager.modules_list();
            let interface_info = InterfaceInfo {
                bot_name: self.0.bot_name.clone(),
                root_path: self.0.root_path.clone(),
                loaded_crates,
            };
            let interface = Interface::new(interface_info)?;

            self.0.events.activate_handle(SylphieEvents {
                root_module,
                module_manager,
                interface: interface.clone(),
            });

            interface.start(&self.0.events.lock())
        } else {
            panic!("SylphieCore has already been started.")
        }
    }

    pub fn get_handler(&self) -> Option<Handler<SylphieEvents<R>>> {
        self.0.events.try_lock()
    }

    // TODO: Shutdown
}
impl <R: Module> Clone for SylphieCore<R> {
    fn clone(&self) -> Self {
        SylphieCore(self.0.clone())
    }
}