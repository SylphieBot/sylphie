use crate::errors::*;
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
}

impl <R: Module> SylphieEvents<R> {
    fn new(core: SylphieCore<R>) -> SylphieEvents<R> {
        let (module_manager, root_module) = ModuleManager::init(core);
        SylphieEvents {
            root_module, module_manager,
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

    pub fn start(&self) {
        if !self.0.is_started.compare_and_swap(false, true, Ordering::Relaxed) {
            self.0.events.activate_handle(SylphieEvents::new(self.clone()));
            eprintln!("{:#?}", self.0.events.lock().unwrap().get_service::<ModuleManager>());
        } else {
            panic!("SylphieCore has already been started.")
        }
    }

    pub fn get_handler(&self) -> Option<Handler<SylphieEvents<R>>> {
        self.0.events.lock()
    }

    // TODO: Shutdown
}
impl <R: Module> Clone for SylphieCore<R> {
    fn clone(&self) -> Self {
        SylphieCore(self.0.clone())
    }
}