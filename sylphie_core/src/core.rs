use crate::database::*;
use crate::errors::*;
use crate::interface::*;
use crate::module::{Module, ModuleManager};
use fs2::*;
use static_events::*;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

static CORE_STARTED: AtomicBool = AtomicBool::new(false);

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

/// Dispatched when the bot is started, before user interface is initialized.
#[derive(Copy, Clone, Debug)]
pub struct InitEvent;
simple_event!(InitEvent);

/// Dispatched after shutdown is initialized, and after the user interface is killed.
#[derive(Copy, Clone, Debug)]
pub struct ShutdownEvent;
simple_event!(ShutdownEvent);

struct ShutdownStartedEvent;
simple_event!(ShutdownStartedEvent);

/// The [`Events`] implementation used for a particular [`SylphieCore`].
#[derive(Events)]
pub struct SylphieEvents<R: Module> {
    #[subhandler] root_module: R,
    #[service] module_manager: ModuleManager,
    #[service] interface: Interface,
    #[service] database: Database,
    #[service] core_ref: CoreRef<R>,
}

#[events_impl]
impl <R: Module> SylphieEvents<R> {
    #[event_handler]
    fn builtin_commands(
        &self, target: &Handler<impl Events>, command: &TerminalCommandEvent,
    ) -> EventResult {
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
            ".shutdown" => target.shutdown_bot(),
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
            _ => return EvOk
        }
        EvCancel
    }

    #[event_handler(EvAfterEvent)]
    fn unknown_terminal_command(&self, _: &TerminalCommandEvent) {
        error!("Unknown command.");
    }

    #[event_handler]
    fn shutdown_handler(&self, _: &ShutdownStartedEvent) {
        self.interface.shutdown();
    }
}

/// A handle that allows operations to be performed on the bot outside the events loop.
#[derive(Clone)]
pub struct CoreRef<R: Module>(EventsHandle<SylphieEvents<R>>);
impl <R: Module> CoreRef<R> {
    // Gets whether the bot has been shut down.
    pub fn is_shutdown(&self) -> bool {
        self.0.is_shutdown()
    }

    /// Gets the number of active handlers from this handle, or handles cloned from it.
    pub fn lock_count(&self) -> usize {
        self.0.lock_count()
    }

    /// Returns the underlying [`Handler`], or panics if the bot has already been shut down.
    pub fn lock(&self) -> Handler<SylphieEvents<R>> {
        self.0.lock()
    }

    /// Returns the underlying [`Handler`] wrapped in a [`Some`], or [`None`] if the bot has
    /// already been shut down.
    pub fn try_lock(&self) -> Option<Handler<SylphieEvents<R>>> {
        self.0.try_lock()
    }

}

pub struct SylphieCore<R: Module> {
    bot_name: String,
    root_path: PathBuf,
    events: EventsHandle<SylphieEvents<R>>,
}
impl <R: Module> SylphieCore<R> {
    pub fn new(bot_name: impl Into<String>) -> Self {
        SylphieCore {
            bot_name: bot_name.into(),
            root_path: get_root_path(),
            events: EventsHandle::new(),
        }
    }

    fn db_root(&self) -> Result<PathBuf> {
        let mut root_path = self.root_path.clone();
        root_path.push("db");
        if !root_path.is_dir() {
            fs::create_dir_all(&root_path)?;
        }
        Ok(root_path)
    }
    fn lock(&mut self) -> Result<File> {
        let mut lock_path = self.db_root()?;
        lock_path.push(format!("{}.lock", &self.bot_name));
        check_lock(lock_path)
    }
    fn init_db(&self) -> Result<Database> {
        let root_path = self.db_root()?;
        let mut db_path = root_path.clone();
        db_path.push(format!("{}.db", &self.bot_name));
        let mut transient_path = root_path;
        transient_path.push(format!("{}.transient.db", &self.bot_name));

        Database::new(db_path, transient_path)
    }

    /// Starts the bot core, blocking the main thread until the bot returns.
    ///
    /// This sets loggers with `tracing` and `log`. You will need your own log subscribers to
    /// log messages before calling this function. In addition, this function will panic if you
    /// have set a `log` logger before calling this function.
    ///
    /// This sets the panic hook to allow for better error reporting.
    ///
    /// # Panics
    ///
    /// Only one bot core may be started in the lifetime of a process. Any started after the
    /// first will immediately panic.
    pub fn start(mut self) -> Result<()> {
        if CORE_STARTED.swap(true, Ordering::SeqCst) {
            // TODO: Eventually work on relaxing this restriction to "one at once."
            panic!("Only one bot core may be started in the lifetime of a process.");
        }

        early_init();

        let _lock = self.lock()?;

        let (module_manager, root_module) = ModuleManager::init(CoreRef(self.events.clone()));
        let loaded_crates = module_manager.modules_list();
        let interface_info = InterfaceInfo {
            bot_name: self.bot_name.clone(),
            root_path: self.root_path.clone(),
            loaded_crates,
        };
        let interface = Interface::new(interface_info)
            .internal_err(|| "Could not initialize user interface.")?;

        self.events.activate_handle(SylphieEvents {
            root_module,
            module_manager,
            interface: interface.clone(),
            database: self.init_db().internal_err(|| "Could not initialize database.")?,
            core_ref: CoreRef(self.events.clone()),
        });
        interface.start(&self.events.lock())?;
        self.events.lock().dispatch(ShutdownEvent);
        self.events.shutdown(); // TODO: shutdown with progress

        Ok(())
    }
}

pub struct SylphieHandlerExtCore<'a, E: Events>(&'a Handler<E>);

/// Contains extension functions defined directly on `Handler<impl Events>`.
///
/// This is the main way to access a lot of core bot functionality. Most of the functions in this
/// trait will panic if called on a handler that is not based on Sylphie.
pub trait SylphieHandlerExt {
    /// Shuts down the bot.
    fn shutdown_bot(&self);

    /// Returns a connection to the database.
    fn connect_db(&self) -> Result<DatabaseConnection>;
}
impl <E: Events> SylphieHandlerExt for Handler<E> {
    fn shutdown_bot(&self) {
        self.dispatch(ShutdownStartedEvent);
    }

    fn connect_db(&self) -> Result<DatabaseConnection> {
        self.get_service::<Database>().connect()
    }
}

/// Initializes the compatibility layer between `log` and `tracing`, the fallback logger, and the
/// panic hook allowing [`Error::catch_panic`] to work correctly.
///
/// This may be called multiple times without errors. However, it will set a logger to the
/// `log` crate, and will panic if another has already been set.
pub fn early_init() {
    crate::interface::init_early_logging();
    crate::errors::activate_panic_hook();
}