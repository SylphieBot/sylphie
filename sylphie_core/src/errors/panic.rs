//! Handles storing panic info into [`Error`] structs.

use backtrace::Backtrace;
use std::any::Any;
use std::borrow::Cow;
use std::cell::RefCell;
use std::future::*;
use std::panic::{self, *};
use std::pin::Pin;
use std::sync::Once;
use std::task::{Context, Poll};

/// The location a panic occurred at.
#[derive(Clone, Debug)]
pub struct PanicLocation {
    /// The file the panic occurred at.
    pub file: String,
    /// The line the panic occurred at.
    pub line: u32,
    /// The column the panic occurred at.
    pub col: u32,
}

pub struct PanicInfo {
    pub payload: Cow<'static, str>,
    pub panic_loc: Option<PanicLocation>,
    /// Whether the backtrace is from the catch_panic site instead of the panic hook.
    pub backtrace: Backtrace,
}

struct PanicInfoStore {
    active_count: usize,
    info: Option<PanicInfo>,
}

thread_local!(static PANIC_INFO: RefCell<PanicInfoStore> = RefCell::new(PanicInfoStore {
    active_count: 0,
    info: None,
}));

fn payload_to_str(payload: &(dyn Any + Send)) -> Cow<'static, str> {
    if let Some(x) = payload.downcast_ref::<&'static str>() {
        (*x).into()
    } else if let Some(x) = payload.downcast_ref::<String>() {
        x.clone().into()
    } else {
        "<unknown panic payload>".into()
    }
}
fn panic_hook(info: &panic::PanicInfo<'_>) {
    let payload = payload_to_str(info.payload());
    let panic_loc = info.location().map(|x| {
        PanicLocation {
            file: x.file().to_string(),
            line: x.line(),
            col: x.column(),
        }
    });
    let info = PanicInfo {
        payload,
        panic_loc,
        backtrace: Backtrace::new(),
    };
    PANIC_INFO.with(|info_ref| info_ref.borrow_mut().info = Some(info));
}
pub fn activate_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |x| {
            if PANIC_INFO.with(|info_ref| info_ref.borrow().active_count) > 0 {
                panic_hook(x)
            } else {
                default_hook(x)
            }
        }));
    });
}

pub fn catch_unwind<T>(f: impl FnOnce() -> T) -> Result<T, PanicInfo> {
    PANIC_INFO.with(|info_ref| {
        let mut borrow = info_ref.borrow_mut();
        borrow.info = None; // safeguard, not sure how this can happen
        borrow.active_count += 1;
    });
    let result = match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(x) => Ok(x),
        Err(raw_info) => {
            PANIC_INFO.with(|info_ref| {
                let info = info_ref.borrow_mut().info.take();
                Err(if info.is_some() {
                    info.unwrap()
                } else {
                    PanicInfo {
                        payload: payload_to_str(&raw_info),
                        panic_loc: None,
                        backtrace: Backtrace::new(),
                    }
                })
            })
        },
    };
    PANIC_INFO.with(|info_ref| {
        let mut borrow = info_ref.borrow_mut();
        borrow.info = None; // safeguard, this can happen if a panic is caught by something else.
        borrow.active_count -= 1;
    });
    result
}

pub struct CatchUnwind<Fut: Future>(pub Fut);
impl <Fut: Future> Future for CatchUnwind<Fut> {
    type Output = Result<Fut::Output, PanicInfo>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let f = unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().0) };
        catch_unwind(|| Fut::poll(f, cx))?.map(Ok)
    }
}