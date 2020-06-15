//! Handles generating error reports and detecting deadlocks.

use backtrace::Backtrace;
use crate::errors::*;
use crate::interface::InterfaceShared;
use crate::utils::*;
use chrono::Utc;
use lazy_static::*;
use parking_lot::Once;
use parking_lot::deadlock;
use std::collections::BTreeMap;
use std::fmt::{self, Formatter};
use std::fs;
use std::fs::File;
use std::io::{Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

struct ThreadName;
impl fmt::Display for ThreadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(thread::current().name().unwrap_or("<unknown>"))
    }
}

// the type from parking_lot is a Voldemort type
// this mainly exists so we can have an owned version
struct DeadlockInfo {
    thread_id: usize,
    backtrace: Backtrace,
}
fn check_deadlock() -> Vec<Vec<DeadlockInfo>> {
    let raw = deadlock::check_deadlock();
    raw.into_iter().map(|x| x.into_iter().map(|x| DeadlockInfo {
        thread_id: x.thread_id(),
        backtrace: x.backtrace().clone(),
    }).collect()).collect()
}

lazy_static! {
    static ref CURRENT_CTX: GlobalInstance<ErrorCtx> = GlobalInstance::new();
}

#[derive(Clone)]
pub struct ErrorCtx(Arc<InterfaceShared>);
impl ErrorCtx {
    pub(in super) fn new(shared: Arc<InterfaceShared>) -> Self {
        ErrorCtx(shared)
    }
    pub fn activate(self) -> InstanceScopeGuard<ErrorCtx> {
        CURRENT_CTX.set_instance(self)
    }

    fn fmt_header(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "--- {} Error Report ---\n\n", &self.0.info.bot_name)?;

        // write loaded modules information
        if let Some(packages) = &*self.0.loaded_crates.load() {
            write!(fmt, "Loaded packages:\n")?;
            for module in &***packages {
                write!(fmt, "    {} {}", module.crate_path, module.crate_version)?;
                if let Some(git_info) = &module.git_info {
                    write!(fmt, " ({}, r{}", git_info.name, if git_info.revision.len() > 8 {
                        &git_info.revision[..8]
                    } else {
                        git_info.revision
                    })?;
                    if git_info.modified_files > 0 {
                        write!(fmt, ", {} dirty files", git_info.modified_files)?;
                    }
                    write!(fmt, ")")?;
                }
                writeln!(fmt)?;
            }
        } else {
            write!(fmt, "Loaded packages: (module tree not yet initialized)")?;
        }

        // write platform information
        write!(fmt, concat!("Platform: ", env!("TARGET")))?;
        if env!("TARGET") != env!("HOST") {
            write!(fmt, concat!(", cross-compiled from ", env!("HOST")))?;
        }
        if env!("PROFILE") != "release" {
            write!(fmt, concat!(", ", env!("PROFILE")))?;
        }
        write!(fmt, concat!("\nCompiler: ", env!("RUSTC_VERSION_STR"), "\n"))?;

        Ok(())
    }
    fn fmt_logs(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        // TODO: Logs
        Ok(())
    }
}
fn fmt_error(fmt: &mut fmt::Formatter<'_>, e: &Error) -> fmt::Result {
    write!(fmt, "\nThread '{}' encountered an error: {}\n", ThreadName, e)?;
    let mut current = e.source();
    while let Some(source) = current {
        write!(fmt, "Caused by: {}\n", e)?;
        current = source.source();
    }
    match e.backtrace() {
        Some(bt) => write!(fmt, "\n{:?}", bt)?,
        None => write!(fmt, "\n(from catch site)\n{:?}\n\n", Backtrace::new())?,
    }
    Ok(())
}
fn fmt_deadlock(fmt: &mut fmt::Formatter<'_>, data: &Vec<Vec<DeadlockInfo>>) -> fmt::Result {
    let mut all_thread_buf = BTreeMap::new();
    for deadlock in data {
        for thread in deadlock {
            all_thread_buf.insert(thread.thread_id, &thread.backtrace);
        }
    }

    assert!(!data.is_empty());
    write!(
        fmt, "\nFatal error: {} deadlocks detected in {} threads.\n",
        data.len(), all_thread_buf.len(),
    )?;
    for deadlock in data {
        write!(fmt, " - Deadlock involving {} threads: ", deadlock.len())?;
        let mut is_first = true;
        for thread in deadlock {
            if !is_first {
                write!(fmt, ", ")?;
            }
            is_first = false;
            write!(fmt, "#{}", thread.thread_id)?;
        }
        write!(fmt, "\n")?;
    }
    write!(fmt, "\n")?;

    for (id, backtrace) in all_thread_buf {
        write!(fmt, "(thread #{})\n{:?}\n\n", id, backtrace)?;
    }

    Ok(())
}

fn write_report_file(logs_path: &Path, report: &str) -> Result<PathBuf> {
    let mut path = PathBuf::from(logs_path);
    fs::create_dir_all(&path)?;
    let file_name = format!("error_report_{}.log", Utc::now().format("%Y-%m-%d_%H%M%S%f"));
    path.push(file_name);

    let mut out = File::create(&path)?;
    #[cfg(windows)] out.write_all(report.replace("\n", "\r\n").as_bytes())?;
    #[cfg(not(windows))] out.write_all(report.as_bytes())?;

    Ok(path)
}

fn write_report(lock: InstanceGuard<ErrorCtx>, report: &str) -> Result<()> {
    if !lock.is_loaded() {
        error!("Error encounted during startup/shutdown:\n{}", report);
    } else {
        struct FormatErrorReport<'a>(&'a ErrorCtx, &'a str);
        impl <'a> fmt::Display for FormatErrorReport<'a> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                self.0.fmt_header(f)?;
                f.write_str(self.1)?;
                self.0.fmt_logs(f)?;
                Ok(())
            }
        }

        let full_error = FormatErrorReport(&*lock, report).to_string();
        if let Some(line) = report.trim().split('\n').next() {
            error!("{}", line);
        }

        // TODO: Better logs dir handling
        let mut logs_dir = lock.0.info.root_path.clone();
        logs_dir.push("logs");
        let report_file = write_report_file(&logs_dir, &full_error)?;
        error!(
            "Detailed information about this error can be found at '{}'.", report_file.display(),
        );
        // TODO: Proper way to handle error reporting URLs.
        error!("This is probably a bug. Please report it at [TODO] and include the error report.");
    }
    Ok(())
}

pub fn init_deadlock_detection() {
    struct FormatDeadlock<'a>(&'a Vec<Vec<DeadlockInfo>>);
    impl <'a> fmt::Display for FormatDeadlock<'a> {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            fmt_deadlock(f, &self.0)
        }
    }

    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        thread::Builder::new().name("deadlock detection thread".to_owned()).spawn(|| {
            loop {
                thread::sleep(Duration::from_secs(10));
                let deadlock = check_deadlock();
                if !deadlock.is_empty() {
                    if let Err(e) = write_report(
                        CURRENT_CTX.load(), &FormatDeadlock(&deadlock).to_string(),
                    ) {
                        error!("Error while reporting deadlock: {}", e);
                    }
                    std::process::abort();
                }
            }
        }).expect("failed to start deadlock detection thread");
    });
}
pub fn report_error(err: &Error) {
    struct FormatError<'a>(&'a Error);
    impl <'a> fmt::Display for FormatError<'a> {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            fmt_error(f, self.0)
        }
    }

    if let Err(e) = write_report(
        CURRENT_CTX.load(), &FormatError(err).to_string(),
    ) {
        error!("Error while reporting error: {}", e);
    }
}
