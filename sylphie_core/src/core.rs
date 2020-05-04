use crate::database::*;
use crate::errors::*;
use crate::interface::*;
use crate::module::{Module, ModuleManager};
use fs2::*;
use parking_lot::Mutex;
use static_events::*;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// TODO: Introduce builder for SylphieCore.

fn check_lock(path: impl AsRef<Path>) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    let lock_file = options.open(path)
        .internal_err(|| "Could not open lock file")?;
    lock_file.try_lock_exclusive()
        .internal_err(|| "Could not acquire exclusive lock on database.")?;
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
    #[service] core: SylphieCore<R>,
    #[service] database: Database,
}

#[events_impl]
impl <R: Module> SylphieEvents<R> {
    #[event_handler]
    fn builtin_commands(&self, command: &TerminalCommandEvent) {
        match command.0.as_str().trim() {
            ".help" => {
                info!("Built-in commands:");
                info!(".help - Shows this help message.");
                info!(".info - Prints information about the bot.");
                info!(".shutdown - Shuts down the bot.");
                info!(".abort!! - Forcefully shuts down the bot.");
            }
            ".info" => {
                // TODO: Implement.
            }
            ".shutdown" => {
                info!("(shutdown)");
                ::std::process::abort()
            }
            ".abort!!" => {
                eprintln!("(abort)");
                ::std::process::abort()
            }
            x if x.starts_with(".abort") => {
                info!("Please use '.abort!!' if you really mean to forcefully stop the bot.");
            }
            x if x.starts_with('.') => {
                info!("Unknown built-in command. Use '.help' for more information.");
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
    lock: Mutex<Option<File>>,
}

pub struct SylphieCore<R: Module>(Arc<CoreData<R>>);
impl <R: Module> SylphieCore<R> {
    pub fn new(bot_name: impl Into<String>) -> Self {
        SylphieCore(Arc::new(CoreData {
            is_started: AtomicBool::new(false),
            bot_name: bot_name.into(),
            root_path: get_root_path(),
            events: EventsHandle::new(),
            lock: Mutex::new(None),
        }))
    }

    fn db_root(&self) -> Result<PathBuf> {
        let mut root_path = self.0.root_path.clone();
        root_path.push("db");
        if !root_path.is_dir() {
            fs::create_dir_all(&root_path)?;
        }
        Ok(root_path)
    }
    fn lock(&self) -> Result<()> {
        let mut lock = self.0.lock.lock();
        if lock.is_none() {
            let mut lock_path = self.db_root()?;
            lock_path.push(format!("{}.lock", &self.0.bot_name));
            *lock = Some(check_lock(lock_path)?);
        }
        Ok(())
    }
    fn init_db(&self) -> Result<Database> {
        let root_path = self.db_root()?;
        let mut db_path = root_path.clone();
        db_path.push(format!("{}.db", &self.0.bot_name));
        let mut transient_path = root_path;
        transient_path.push(format!("{}.transient.db", &self.0.bot_name));

        Database::new(db_path, transient_path)
    }

    /// Starts the bot core, blocking the main thread until the bot returns.
    pub fn start(&self) -> Result<()> {
        if !self.0.is_started.compare_and_swap(false, true, Ordering::Relaxed) {
            self.lock()?;

            let (module_manager, root_module) = ModuleManager::init(self.clone());
            let loaded_crates = module_manager.modules_list();
            let interface_info = InterfaceInfo {
                bot_name: self.0.bot_name.clone(),
                root_path: self.0.root_path.clone(),
                loaded_crates,
            };
            let interface = Interface::new(interface_info)
                .internal_err(|| "Could not initialize user interface.")?;

            self.0.events.activate_handle(SylphieEvents {
                root_module,
                module_manager,
                interface: interface.clone(),
                core: self.clone(),
                database: self.init_db().internal_err(|| "Could not initialize database.")?,
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

/// Contains convenience functions defined directly on `Handler<impl Events>`.
pub trait SylphieHandlerExt {
    /// Returns a connection to the database.
    fn connect_db(&self) -> Result<DatabaseConnection>;
}
impl <E: Events> SylphieHandlerExt for Handler<E> {
    fn connect_db(&self) -> Result<DatabaseConnection> {
        self.get_service::<Database>().connect()
    }
}