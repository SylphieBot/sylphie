use crate::errors::*;
use crate::global_instance::*;
use crate::interface::*;
use crate::module::{Module, ModuleManager};
use fs2::*;
use lazy_static::*;
use static_events::prelude_async::*;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::marker::PhantomData;
use std::thread;
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

/// Dispatched when the bot is started, before [`InitEvent`].
///
/// This event is dispatched synchronously.
pub struct EarlyInitEvent(());
failable_event!(EarlyInitEvent, (), Error);

/// Dispatched when the bot is started, before user interface is initialized.
pub struct InitEvent(());
failable_event!(InitEvent, (), Error);

/// Dispatched after shutdown is initialized, and after the user interface is killed.
pub struct ShutdownEvent(());
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
    #[service] bot_info: BotInfo,
}

lazy_static! {
    static ref SYLPHIE_RUNNING_GUARD: GlobalInstance<()> = GlobalInstance::new();
}

/// Stores information related to the bot.
///
/// This can be retrieved using `get_service`.
#[derive(Clone)]
pub struct BotInfo {
    bot_name: String,
    root_path: PathBuf,
}
impl BotInfo {
    /// Returns the name of the bot.
    pub fn bot_name(&self) -> &str {
        &self.bot_name
    }

    /// Returns the path where the bot's state is stored.
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }
}

pub struct SylphieCore<R: Module> {
    info: BotInfo,
    phantom: PhantomData<R>,
}
impl <R: Module> SylphieCore<R> {
    pub fn new(bot_name: impl Into<String>) -> Self {
        let mut root_path = get_root_path();
        root_path.push("run");
        SylphieCore {
            info: BotInfo {
                bot_name: bot_name.into(),
                root_path,
            },
            phantom: PhantomData,
        }
    }
    fn lock(&mut self) -> Result<File> {
        let mut lock_path = self.info.root_path.clone();
        if !lock_path.is_dir() {
            fs::create_dir_all(&lock_path)?;
        }
        lock_path.push(".lock");
        check_lock(lock_path)
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
    /// Only one bot core may be started at one time. Any cores started while another core is
    /// running
    pub fn start(mut self) -> Result<()> {
        // acquire the per-process lock
        let _guard = SYLPHIE_RUNNING_GUARD.set_instance(());

        // initialize early logging and related processes
        early_init();

        // acquire the database lock
        let _lock = self.lock()?;

        // initializes the tokio runtime
        let runtime = tokio::runtime::Builder::new()
            .threaded_scheduler()
            .enable_all()
            .build()?;
        runtime.enter(move || -> Result<()> {
            let runtime = tokio::runtime::Handle::current();

            // initialize the interface system
            let interface_info = InterfaceInfo {
                bot_name: self.info.bot_name.clone(),
                root_path: self.info.root_path.clone(),
            };
            let interface = Interface::new(interface_info)
                .internal_err(|| "Could not initialize user interface.")?;

            // initialize the module tree and events dispatch
            let (module_manager, root_module) = ModuleManager::init::<R>();
            interface.set_loaded_crates(module_manager.loaded_crates_list());
            let handler = Handler::new(SylphieEvents {
                root_module,
                events: events::SylphieEventsImpl(PhantomData),
                module_manager,
                interface: interface.clone(),
                bot_info: self.info.clone(),
            });

            // start the actual bot itself
            handler.dispatch_sync(EarlyInitEvent(()))?;
            runtime.block_on(handler.dispatch_async(InitEvent(())))?;
            interface.start(&handler)?;
            runtime.block_on(handler.dispatch_async(ShutdownEvent(())));

            // wait for shutdown
            let mut ct = 0;
            while handler.refcount() > 1 {
                if (ct % 500) == 100 {
                    info!(
                        "Waiting on {} threads to stop. Press {}+C to force shutdown.",
                        handler.refcount() - 1,
                        if env!("TARGET").contains("apple-darwin") { "Command" } else { "Ctrl" },
                    );
                }
                ct += 1;
                thread::sleep(Duration::from_millis(10));
            }

            Ok(())
        })?;
        Ok(())
    }
}

/// Contains extension functions defined directly on `Handler<impl Events>`.
///
/// This is the main way to access a lot of core bot functionality. Most of the functions in this
/// trait will panic if called on a handler that is not based on Sylphie.
pub trait SylphieCoreHandlerExt {
    /// Shuts down the bot.
    fn shutdown_bot(&self);
}
impl <E: Events> SylphieCoreHandlerExt for Handler<E> {
    fn shutdown_bot(&self) {
        self.dispatch_sync(ShutdownStartedEvent);
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