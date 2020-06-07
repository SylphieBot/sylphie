//! Handles generating error reports and detecting deadlocks.

use backtrace::Backtrace;
use crate::errors::*;
use crate::interface::InterfaceShared;
use crate::module::*;
use chrono::Utc;
use failure::Fail;
use parking_lot::RwLock;
use parking_lot::deadlock::check_deadlock;
use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Write as FmtWrite};
use std::fs;
use std::fs::File;
use std::io::{Write as IoWrite};
use std::panic::*;
use std::path::{Path, PathBuf};
use std::process::abort;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Copy, Clone, Debug)]
enum ReportType {
    Error, Panic, Deadlock,
}
impl ReportType {
    fn name(self) -> &'static str {
        match self {
            ReportType::Error    => "Error",
            ReportType::Panic    => "Panic",
            ReportType::Deadlock => "Deadlock",
        }
    }
    fn lc_name(self) -> &'static str {
        match self {
            ReportType::Error    => "error",
            ReportType::Panic    => "panic",
            ReportType::Deadlock => "deadlock",
        }
    }
}

struct ThreadName;
impl fmt::Display for ThreadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(thread::current().name().unwrap_or("<unknown>"))
    }
}

#[derive(Clone)]
pub struct ErrorCtx(Arc<InterfaceShared>);
impl ErrorCtx {
    pub(in super) fn new(shared: Arc<InterfaceShared>) -> Self {
        ErrorCtx(shared)
    }

    fn fmt_header(&self, fmt: &mut fmt::Formatter<'_>, kind: ReportType) -> fmt::Result {
        write!(fmt, "--- {} {} Report ---\n\n", &self.0.info.bot_name, kind.name())?;

        // write loaded modules information
        write!(fmt, "Loaded packages:\n")?;
        for module in &*self.0.info.loaded_crates {
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
        Ok(())
    }

    pub fn fmt_error_report(&self, fmt: &mut fmt::Formatter<'_>, e: &dyn Fail) -> fmt::Result {
        self.fmt_header(fmt, ReportType::Error)?;
        write!(fmt, "\nThread '{}' encountered an error: {}\n", ThreadName, e)?;
        for e in e.iter_causes() {
            write!(fmt, "Caused by: {}\n", e)?;
        }
        match e.backtrace() {
            Some(bt) => write!(fmt, "\n{}\n\n", bt)?,
            None => write!(fmt, "\n(from catch site)\n{:?}\n\n", Backtrace::new())?,
        }
        self.fmt_logs(fmt)?;
        Ok(())
    }
    pub fn fmt_panic_report(
        &self, fmt: &mut fmt::Formatter<'_>,
        info: &(dyn Any + Send), loc: Option<&Location>, backtrace: &Backtrace,
    ) -> fmt::Result {
        todo!()
    }
}

/*
fn write_report_file(root_path: impl AsRef<Path>, kind: &str, report: &str) -> Result<PathBuf> {
    let mut path = PathBuf::from(root_path.as_ref());
    path.push("logs");
    fs::create_dir_all(&path)?;
    let file_name = format!("{}_report_{}.log", kind, Utc::now().format("%Y%m%d_%H%M%S%f"));
    path.push(file_name);

    let mut out = File::create(&path)?;
    #[cfg(windows)] out.write_all(report.replace("\n", "\r\n").as_bytes())?;
    #[cfg(not(windows))] out.write_all(report.as_bytes())?;

    Ok(path)
}

static ROOT_PATH: RwLock<Option<PathBuf>> = RwLock::new(None);
fn write_report(kind: ReportType, cause: &str, backtrace: &str) -> Result<()> {
    if let Some(line) = cause.trim().split('\n').next() {
        error!("{}", line);
    }

    let root_path = ROOT_PATH.read().as_ref().unwrap().clone();
    let report_file = write_report_file(root_path, kind.lc_name(),
                                        &make_error_report(kind, cause, backtrace)?)?;
    error!("Detailed information about this {} can be found at '{}'.",
           kind.lc_name(), report_file.display());
    error!("This is probably a bug. Please report it at \
            https://github.com/Lymia/sylph-verifier/issues and include the {} report.",
           kind.lc_name());
    Ok(())
}

fn check_report_deadlock() -> Result<bool> {
    let deadlocks = check_deadlock();
    if !deadlocks.is_empty() {
        let mut serial_id = 1;
        let mut bt_keys = Vec::new();
        let mut bt_ids = HashMap::new();
        let mut bt_map = HashMap::new();
        for deadlock in &deadlocks {
            for thread in deadlock {
                if !bt_map.contains_key(&thread.thread_id()) {
                    bt_keys.push(thread.thread_id());
                    bt_ids.insert(thread.thread_id(), serial_id);
                    bt_map.insert(thread.thread_id(), thread.backtrace());
                    serial_id += 1;
                }
            }
        }

        let mut cause = String::new();
        writeln!(cause, "{} deadlock(s) detected in {} threads!", deadlocks.len(), bt_map.len())?;
        for deadlock in &deadlocks {
            let mut threads_str = String::new();
            let mut is_first = true;
            for thread in deadlock {
                if !is_first {
                    write!(threads_str, ", ")?;
                }
                is_first = false;
                write!(threads_str, "{}", bt_ids[&thread.thread_id()])?;
            }
            writeln!(cause, "Deadlock involving {} threads: {}", deadlock.len(), threads_str)?;
        }

        let mut backtrace = String::new();
        for key in bt_keys {
            writeln!(backtrace, "(thread #{})\n{:?}\n",
                     bt_ids[&key], bt_map[&key])?;
        }

        logger::lock_log_sender();
        println!();
        write_report(ReportType::Deadlock, &cause, &backtrace)?;

        Ok(true)
    } else {
        Ok(false)
    }
}

fn report_err(e: &impl Fail) -> Result<()> {
    let mut cause = String::new();
    writeln!(cause, "Thread {} errored with '{}'", thread_name(), e)?;
    for e in e.causes().skip(1) {
        writeln!(cause, "Caused by: {}", e)?;
    }

    let backtrace = match e.backtrace() {
        Some(bt) => format!("{}", bt),
        None => format!("(from catch site)\n{:?}", Backtrace::new()),
    };
    write_report(ReportType::Error, &cause, &backtrace)?;
    Ok(())
}
fn cause_from_panic(info: &(dyn Any + Send), loc: Option<&Location>) -> String {
    let raw_cause: Cow<'static, str> = if let Some(&s) = info.downcast_ref::<&str>() {
        format!("'{}'", s).into()
    } else if let Some(s) = info.downcast_ref::<String>() {
        format!("'{}'", s).into()
    } else {
        "unknown panic information".into()
    };
    let raw_location: Cow<'static, str> = loc.map_or("".into(), |loc| {
        format!(" at {}:{}", loc.file(), loc.line()).into()
    });
    format!("Thread '{}' panicked with {}{}", thread_name(), raw_cause, raw_location)
}
pub fn init(root_path: impl AsRef<Path>) {
    *ROOT_PATH.write() = Some(root_path.as_ref().to_owned());

    set_hook(Box::new(|panic_info| {
        let cause = cause_from_panic(panic_info.payload(), panic_info.location());
        let backtrace = format!("{:?}", Backtrace::new());
        write_report(ReportType::Panic, &cause, &backtrace).expect("failed to write panic report!");
    }));

    thread::Builder::new().name("deadlock detection thread".to_owned()).spawn(|| {
        loop {
            thread::sleep(Duration::from_secs(10));
            match check_report_deadlock() {
                Ok(false) => { }
                Ok(true) => abort(),
                Err(e) => {
                    logger::lock_log_sender();
                    println!();
                    report_err(&e).ok();
                    abort();
                }
            }
        }
    }).expect("failed to start deadlock detection thread");
}

pub fn catch_error<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(t)) => Ok(t),
        Ok(Err(e)) => {
            report_err(&e)?;
            Err(e)
        }
        Err(_) => Err(ErrorKind::Panicked.into()),
    }
}
*/