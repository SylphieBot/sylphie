use crate::database::*;
use crate::errors::*;
use crate::interface::*;
use crate::module::{Module, ModuleManager};
use crate::utils::GlobalInstance;
use fs2::*;
use lazy_static::*;
use static_events::prelude_async::*;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::marker::PhantomData;
use std::time::Duration;

mod events;

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
fn get_dir_from_cargo(path: PathBuf) -> Option<PathBuf> {
    // Check for other cargo-related env vars to be safe.
    if env::var_os("CARGO").is_none() ||
        env::var_os("CARGO_PKG_NAME").is_none() ||
        env::var_os("CARGO_PKG_VERSION").is_none()
    {
        return None
    }

    // Check for a Cargo.toml
    let mut cur_path = path.clone();
    cur_path.push("Cargo.toml");
    if !(cur_path.exists() || cur_path.is_file()) {
        return None
    }
    cur_path.pop();
    cur_path.push(".git");
    if cur_path.exists() && cur_path.is_dir() {
        // We found a .git directory. Assume there is no workspace setup.
        return None
    }

    // Check for the most typical workspace setup.
    cur_path.pop();
    cur_path.pop();
    cur_path.push("Cargo.toml");
    if cur_path.exists() && cur_path.is_file() {
        cur_path.pop();
        Some(cur_path)
    } else {
        Some(path)
    }
}
fn get_root_path() -> PathBuf {
    env::var_os("CARGO_MANIFEST_DIR")
        .and_then(|x| get_dir_from_cargo(PathBuf::from(x)))
        .unwrap_or_else(|| get_exe_dir())
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
    #[subhandler] events: events::SylphieEventsImpl<R>,
    #[service] module_manager: ModuleManager,
    #[service] interface: Interface,
    #[service] database: Database,
    #[service] core_ref: CoreRef<R>,
}

/// A handle that allows operations to be performed on the bot outside the events loop.
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
impl <R: Module> Clone for CoreRef<R> {
    fn clone(&self) -> Self {
        CoreRef(self.0.clone())
    }
}

lazy_static! {
    static ref SYLPHIE_RUNNING_GUARD: GlobalInstance<()> = GlobalInstance::new();
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
        // acquire the per-process lock
        let _guard = SYLPHIE_RUNNING_GUARD.set_instance(());

        // initialize early logging and related processes
        early_init();

        // acquire the database lock
        let _lock = self.lock()?;

        // initializes the tokio runtime
        let runtime = tokio::runtime::Builder::new().thread_name("sylphie").build()?;
        runtime.enter(move || -> Result<()> {
            let handle = tokio::runtime::Handle::current();

            // initialize the interface system
            let interface_info = InterfaceInfo {
                bot_name: self.bot_name.clone(),
                root_path: self.root_path.clone(),
            };
            let interface = Interface::new(interface_info)
                .internal_err(|| "Could not initialize user interface.")?;

            // initialize the module tree
            let (module_manager, root_module) =
                ModuleManager::init(CoreRef(self.events.clone()));
            interface.set_loaded_crates(module_manager.modules_list());

            self.events.activate_handle(SylphieEvents {
                root_module,
                events: events::SylphieEventsImpl(PhantomData),
                module_manager,
                interface: interface.clone(),
                database: self.init_db().internal_err(|| "Could not initialize database.")?,
                core_ref: CoreRef(self.events.clone()),
            });
            handle.block_on(self.events.lock().dispatch_async(InitEvent));
            interface.start(&self.events.lock())?;
            handle.block_on(self.events.lock().dispatch_async(ShutdownEvent));

            let mut ct = 0;
            self.events.shutdown_with_progress(Duration::from_secs(1), || {
                if ct % 5 == 1 {
                    info!(
                        "Waiting on {} threads to stop. Press {}+C to force shutdown.",
                        self.events.lock_count(),
                        if env!("TARGET").contains("apple-darwin") { "Command" } else { "Ctrl" },
                    );
                }
                ct += 1;
            });

            Ok(())
        })?;
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
        self.dispatch_sync(ShutdownStartedEvent);
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
    crate::interface::init_interface();
    crate::errors::init_panic_hook();
}