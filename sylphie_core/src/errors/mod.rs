use backtrace::Backtrace;
use minnie_errors::{Error as MinnieError};
use std::borrow::Cow;
use std::error::{Error as StdError};
use std::fmt;
use std::future::Future;
use thiserror::*;

pub(crate) use std::result::{Result as StdResult};

mod panic;
pub use panic::PanicLocation;
pub(crate) use panic::init_panic_hook;

/// The type of error contained within an [`Error`].
#[derive(Error, Debug)]
pub enum ErrorKind {
    /// An internal error occurred.
    #[error("Internal error: {0}")]
    InternalError(Cow<'static, str>),
    /// An panic occurred.
    #[error("Panicked with '{0}' {}", panic::DisplayOptPanicLoc(&.1))]
    Panicked(Cow<'static, str>, Option<PanicLocation>),
    /// An error occurred in a command.
    ///
    /// These errors are meant to be reported to the user and are not internal errors.
    #[error("Command error occurred: {0}")]
    CommandError(Cow<'static, str>),

    /// An error occurred from Minnie.
    #[error("Discord error occurred: {0}")]
    MinnieError(MinnieError),

    /// A wrapped generic error.
    #[error("{0}")]
    GenericError(Box<dyn StdError + Send + 'static>),
    /// An error that does implement [`Send`] and was converted to a string.
    #[error("{0}")]
    NonSendError(String),
}

struct ErrorData {
    kind: ErrorKind,
    backtrace: Option<Backtrace>,
    ctx_backtraces: Vec<Backtrace>,
    cause: Option<Box<dyn StdError + Send + 'static>>,
}

/// The type of error used throughout Sylphie. It allows wrapping arbitrary error types as it is
/// expected to be returned by many functions in users of the bot framework.
///
/// To faciliate this, it does not directly implement [`Fail`]. If you need one, use
/// [`Error::into_fail`].
pub struct Error(Box<ErrorData>);
impl Error {
    /// Creates a new error with no backtrace or cause information.
    #[inline(never)] #[cold]
    pub fn new(kind: ErrorKind) -> Self {
        Error(Box::new(ErrorData {
            kind, backtrace: None, ctx_backtraces: Vec::new(), cause: None,
        }))
    }

    /// Creates a new error with cause information.
    ///
    /// If the given failure is a wrapped [`Error`] containing a [`GenericFail`], the backtrace
    /// and and cause information is directly inlined into this error, instead of through a
    /// wrapper.
    #[inline(never)] #[cold]
    pub fn new_with_cause(kind: ErrorKind, cause: impl StdError + Send + 'static) -> Self {
        Error::new(kind).with_cause(cause)
    }

    /// Creates a new error with backtrace information.
    #[inline(never)] #[cold]
    pub fn new_with_backtrace(kind: ErrorKind) -> Self {
        Error::new(kind).with_backtrace()
    }

    /// Adds backtrace information to this error, if none already exists.
    #[inline(never)] #[cold]
    pub fn with_backtrace(mut self) -> Self {
        if !self.backtrace().is_some() {
            self.0.backtrace = Some(Backtrace::new());
        }
        self
    }

    /// Adds backtrace information to this error, overriding any that may exist.
    #[inline(never)] #[cold]
    pub fn with_context_backtrace(mut self) -> Self {
        self.0.ctx_backtraces.push(Backtrace::new());
        self
    }

    /// Sets the cause information of this error.
    ///
    /// If the given failure is a wrapped [`Error`] containing a [`GenericFail`], the backtrace
    /// and and cause information is directly inlined into this error, instead of through a
    /// wrapper.
    #[inline(never)] #[cold]
    pub fn with_cause(mut self, cause: impl StdError + Send + 'static) -> Self {
        private::BoxFail::set_cause(cause, &mut self);
        self
    }

    #[inline(never)] #[cold]
    fn wrap_panic(panic: panic::PanicInfo) -> Error {
        let mut err = Error::new(ErrorKind::Panicked(panic.payload, panic.panic_loc));
        err.0.backtrace = Some(panic.backtrace);
        err
    }

    /// Catches panics that occur in a closure, wrapping them in an [`Error`].
    #[inline]
    pub fn catch_panic<T>(func: impl FnOnce() -> Result<T>) -> Result<T> {
        match panic::catch_unwind(func) {
            Ok(r) => r,
            Err(panic) => Err(Error::wrap_panic(panic)),
        }
    }

    /// Catches panics that occur in a future, wrapping then in an [`Error`].
    #[inline]
    pub async fn catch_panic_async<T>(fut: impl Future<Output = Result<T>>) -> Result<T> {
        match panic::CatchUnwind(fut).await {
            Ok(v) => v,
            Err(panic) => Err(Error::wrap_panic(panic)),
        }
    }

    /// Returns the type of error contained in this object.
    pub fn error_kind(&self) -> &ErrorKind {
        &self.0.kind
    }

    /// Returns the backtrace associated with this error.
    pub fn backtrace(&self) -> Option<&Backtrace> {
        if let Some(x) = &self.0.backtrace {
            Some(x)
        } else if let ErrorKind::MinnieError(e) = &self.0.kind {
            e.backtrace()
        } else {
            None
        }
    }

    /// Returns backtraces added as additional context.
    pub fn context_backtraces(&self) -> &[Backtrace] {
        &self.0.ctx_backtraces
    }

    /// Gets the source of this error.
    pub fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.0.kind {
            ErrorKind::MinnieError(e) => e.source(),
            ErrorKind::GenericError(f) => f.source(),
            _ => match &self.0.cause {
                Some(x) => Some(&**x),
                _ => None,
            },
        }
    }

    /// Converts this into a [`std::error::Error`].
    pub fn into_std_error(self) -> ErrorWrapper {
        ErrorWrapper(self)
    }
}
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Error")
            .field(&self.0.kind)
            .field(&self.0.cause)
            .finish()
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.kind, f)
    }
}

/// An [`Error`] wrapped in a [`std::error::Error`]
pub struct ErrorWrapper(Error);
impl ErrorWrapper {
    /// Wraps something that can be converted into an [`Error`] into a std error.
    pub fn new(from: impl Into<Error>) -> Self {
        ErrorWrapper(from.into())
    }

    pub fn into_inner(self) -> Error {
        self.0
    }
}
impl StdError for ErrorWrapper {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.0.source()
    }
}
impl From<Error> for ErrorWrapper {
    fn from(e: Error) -> Self {
        ErrorWrapper(e)
    }
}
impl fmt::Debug for ErrorWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
impl fmt::Display for ErrorWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// The result type used throughout the library.
pub type Result<T> = StdResult<T, Error>;

// Contains the internal traits used throughout this module.
mod private {
    use super::*;

    pub trait BoxFail {
        fn set_cause(self, err: &mut Error);
    }
    impl <T: StdError + Send + 'static> BoxFail for T {
        default fn set_cause(self, err: &mut Error) {
            err.0.cause = Some(Box::new(self));
        }
    }
    impl BoxFail for Error {
        fn set_cause(mut self, err: &mut Error) {
            // Only keep a backtrace at the highest level.
            if err.0.backtrace.is_none() {
                err.0.backtrace = self.0.backtrace.take();
            } else {
                self.0.backtrace = None;
            }

            if self.0.cause.is_some() {
                err.0.cause = Some(Box::new(self.into_std_error()));
            } else if let ErrorKind::GenericError(_) = &self.0.kind {
                // collapse the wrapped error into our cause field.
                match self.0.kind {
                    ErrorKind::GenericError(e) => {
                        err.0.cause = Some(e);
                        if err.0.backtrace.is_none() {
                            err.0.backtrace = self.0.backtrace;
                        }
                    }
                    _ => unreachable!(),
                }
            } else {
                err.0.cause = Some(Box::new(self.into_std_error()));
            }
        }
    }
    impl BoxFail for ErrorWrapper {
        fn set_cause(self, err: &mut Error) {
            BoxFail::set_cause(self.0, err)
        }
    }

    pub trait ToError {
        fn into_sylphie_error(self) -> Error;
    }
    impl <T: StdError + Send + 'static> ToError for T {
        default fn into_sylphie_error(self) -> Error {
            Error::new(ErrorKind::GenericError(Box::new(self)))
        }
    }
    impl ToError for ErrorWrapper {
        fn into_sylphie_error(self) -> Error {
            self.0
        }
    }
    impl ToError for MinnieError {
        fn into_sylphie_error(self) -> Error {
            Error::new(ErrorKind::MinnieError(self))
        }
    }

    pub trait ToErrorWithSelf {
        fn into_sylphie_error(self) -> Error;
    }
    impl <T: ToError> ToErrorWithSelf for T {
        fn into_sylphie_error(self) -> Error {
            self.into_sylphie_error()
        }
    }
    impl ToErrorWithSelf for Error {
        fn into_sylphie_error(self) -> Error {
            self
        }
    }

    pub trait ErrorContext<T> {
        fn context(
            self, kind: impl FnOnce() -> ErrorKind, map: impl FnOnce(Error) -> Error,
        ) -> Result<T>;
    }
    impl <T> ErrorContext<T> for Option<T> {
        #[inline]
        fn context(
            self, kind: impl FnOnce() -> ErrorKind, map: impl FnOnce(Error) -> Error,
        ) -> Result<T> {
            #[inline(never)] #[cold]
            fn error_branch<T2>(
                kind: impl FnOnce() -> ErrorKind, map: impl FnOnce(Error) -> Error,
            ) -> Result<T2> {
                Err(map(Error::new(kind())))
            }
            match self {
                Some(x) => Ok(x),
                None => error_branch(kind, map),
            }
        }
    }
    impl <T, E: ToErrorWithSelf> ErrorContext<T> for StdResult<T, E> {
        #[inline]
        fn context(
            self, kind: impl FnOnce() -> ErrorKind, map: impl FnOnce(Error) -> Error,
        ) -> Result<T> {
            #[inline(never)] #[cold]
            fn error_branch<T2, E2: private::ToErrorWithSelf>(
                e: E2, kind: impl FnOnce() -> ErrorKind, map: impl FnOnce(Error) -> Error,
            ) -> Result<T2> {
                Err(map(Error::new_with_cause(kind(), e.into_sylphie_error().into_std_error())))
            }
            match self {
                Ok(x) => Ok(x),
                Err(e) => error_branch(e, kind, map),
            }
        }
    }
}

impl <T: private::ToError> From<T> for Error {
    #[inline(never)] #[cold]
    fn from(err: T) -> Self {
        err.into_sylphie_error().with_backtrace()
    }
}

/// Adds some extra functions to [`Result`] and [`Option`].
pub trait ErrorFromContextExt<T>: Sized {
    /// Appends a new context to the error chain.
    fn context(self, kind: impl FnOnce() -> ErrorKind) -> Result<T>;

    /// Converts an error into a command error.
    ///
    /// When used in a command, Sylphie will display the text directly to the user, instead of
    /// showing an generic error message and logging the error to disk.
    fn cmd_error<S: Into<Cow<'static, str>>>(self, err: impl FnOnce() -> S) -> Result<T>;

    /// Converts an error into an internal error.
    fn internal_err<S: Into<Cow<'static, str>>>(self, err: impl FnOnce() -> S) -> Result<T>;
}
impl <T, U: private::ErrorContext<T>> ErrorFromContextExt<T> for U {
    #[inline]
    fn context(self, kind: impl FnOnce() -> ErrorKind) -> Result<T> {
        private::ErrorContext::context(self, kind, |e| e.with_backtrace())
    }
    #[inline]
    fn cmd_error<S: Into<Cow<'static, str>>>(self, err: impl FnOnce() -> S) -> Result<T> {
        let kind = move || ErrorKind::CommandError(err().into());
        private::ErrorContext::context(self, kind, |e| e)
    }
    #[inline]
    fn internal_err<S: Into<Cow<'static, str>>>(self, err: impl FnOnce() -> S) -> Result<T> {
        let kind = move || ErrorKind::InternalError(err().into());
        private::ErrorContext::context(self, kind, |e| e.with_backtrace())
    }
}

/// Immediately returns with an command error. The function must return an [`Error`], or a type
/// that it can be converted to using [`Into`].
///
/// When used in a command, Sylphie will display the text directly to the user, instead of
/// showing an generic error message and logging the error to disk.
#[macro_export]
macro_rules! cmd_error_493fbda1c52048499126605cd31d3dd3 {
    ($format:expr, $($arg:expr),* $(,)?) => {{
        let text = format!($format, $($arg,)*);
        let err = $crate::errors::Error::new($crate::errors::ErrorKind::CommandError(text.into()));
        return $crate::__macro_export::Err(err.into());
    }};
    ($str:expr) => {{
        let err = $crate::errors::Error::new($crate::errors::ErrorKind::CommandError($str.into()));
        return $crate::__macro_export::Err(err.into());
    }};
}

/// Immediately returns with an internal error. The function must return an [`Error`], or a type
/// that it can be converted to using [`Into`].
#[macro_export]
macro_rules! bail_493fbda1c52048499126605cd31d3dd3 {
    ($format:expr, $($arg:expr),* $(,)?) => {
        return $crate::__macro_export::Err($crate::errors::Error::new_with_backtrace(
            $crate::errors::ErrorKind::InternalError(format!($format, $($arg,)*).into()),
        ).into())
    };
    ($str:expr) => {
        return $crate::__macro_export::Err($crate::errors::Error::new_with_backtrace(
            $crate::errors::ErrorKind::InternalError($str.into()),
        ).into())
    };
}

/// Immediately returns with an internal error if a condition is `false`. The function must
/// return an [`Error`], or a type that it can be converted to using [`Into`].
#[macro_export]
macro_rules! ensure_493fbda1c52048499126605cd31d3dd3 {
    ($check:expr, $($tt:tt)*) => {
        if !$check {
            bail!($($tt)*);
        }
    }
}

#[doc(inline)]
pub use crate::{
    cmd_error_493fbda1c52048499126605cd31d3dd3 as cmd_error,
    bail_493fbda1c52048499126605cd31d3dd3 as bail,
    ensure_493fbda1c52048499126605cd31d3dd3 as ensure,
};

