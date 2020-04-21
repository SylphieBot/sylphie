use failure::*;
use futures::FutureExt;
use minnie::{Error as MinnieError};
use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(crate) use std::result::{Result as StdResult};

/// The type of error contained within an [`Error`].
#[derive(Fail, Debug)]
pub enum ErrorKind {
    /// An internal error occurred.
    #[fail(display = "Internal error: {}", _0)]
    InternalError(Cow<'static, str>),
    /// An panic occurred.
    #[fail(display = "{}", _0)]
    Panicked(Cow<'static, str>),
    #[fail(display = "Command error occurred: {}", _0)]
    CommandError(Cow<'static, str>),

    #[fail(display = "Discord error occurred: {}", _0)]
    MinnieError(MinnieError),

    #[fail(display = "{}", _0)]
    GenericFail(Box<dyn Fail>),
}

struct ErrorData {
    kind: ErrorKind,
    backtrace: Option<Backtrace>,
    cause: Option<Box<dyn Fail>>,
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
            kind, backtrace: None, cause: None
        }))
    }

    /// Creates a new error with cause information.
    ///
    /// If the given failure is a wrapped [`Error`] containing a [`GenericFail`], the backtrace
    /// and and cause information is directly inlined into this error, instead of through a
    /// wrapper.
    #[inline(never)] #[cold]
    pub fn new_with_cause(kind: ErrorKind, cause: impl Fail) -> Self {
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

    /// Sets the cause information of this error.
    ///
    /// If the given failure is a wrapped [`Error`] containing a [`GenericFail`], the backtrace
    /// and and cause information is directly inlined into this error, instead of through a
    /// wrapper.
    #[inline(never)] #[cold]
    pub fn with_cause(mut self, cause: impl Fail) -> Self {
        private::BoxFail::set_cause(cause, &mut self);
        self
    }

    #[inline(never)] #[cold]
    fn wrap_panic(panic: Box<dyn Any + Send + 'static>) -> Error {
        let panic: Cow<'static, str> = if let Some(s) = panic.downcast_ref::<&'static str>() {
            (*s).into()
        } else if let Some(s) = panic.downcast_ref::<String>() {
            s.clone().into()
        } else {
            "<non-string panic info>".into()
        };
        Error::new(ErrorKind::Panicked(panic))
    }

    /// Catches panics that occur in a closure, wrapping them in an [`Error`].
    #[inline]
    pub fn catch_panic<T>(func: impl FnOnce() -> Result<T>) -> Result<T> {
        match catch_unwind(AssertUnwindSafe(func)) {
            Ok(r) => r,
            Err(e) => Err(Error::wrap_panic(e)),
        }
    }

    /// Catches panics that occur in a future, wrapping then in an [`Error`].
    #[inline]
    pub async fn catch_panic_async<T>(fut: impl Future<Output = Result<T>>) -> Result<T> {
        match AssertUnwindSafe(fut).catch_unwind().await {
            Ok(v) => v,
            Err(panic) => Err(Error::wrap_panic(panic)),
        }
    }

    /// Returns the type of error contained in this object.
    pub fn error_kind(&self) -> &ErrorKind {
        &self.0.kind
    }

    /// Finds the first backtrace in the cause chain.
    pub fn find_backtrace(&self) -> Option<&Backtrace> {
        if let Some(x) = &self.0.backtrace {
            Some(x)
        } else {
            let mut current: Option<&dyn Fail> = self.cause();
            while let Some(x) = current {
                if let Some(bt) = x.backtrace() {
                    return Some(bt)
                }
                current = x.cause();
            }
            None
        }
    }

    /// Gets the cause for this error.
    pub fn cause(&self) -> Option<&dyn Fail> {
        match &self.0.kind {
            ErrorKind::MinnieError(e) => e.cause(),
            ErrorKind::GenericFail(f) => f.cause(),
            _ => match self.0.kind.cause() {
                Some(x) => Some(x),
                None => self.0.cause.as_ref().map(|x| &**x),
            }
        }
    }

    /// Gets the backtrace for this
    pub fn backtrace(&self) -> Option<&Backtrace> {
        match &self.0.kind {
            ErrorKind::MinnieError(e) => e.backtrace(),
            ErrorKind::GenericFail(f) => f.backtrace(),
            _ => self.0.backtrace.as_ref(),
        }
    }

    /// Converts this error into a [`Fail`].
    pub fn into_fail(self) -> ErrorAsFail {
        self.into()
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

/// An [`Error`] wrapped in a [`Fail`]
pub struct ErrorAsFail(Error);
impl ErrorAsFail {
    pub fn into_inner(self) -> Error {
        self.0
    }
}
impl Fail for ErrorAsFail {
    fn name(&self) -> Option<&str> {
        Some("sylphie::errors::ErrorAsFail")
    }
    fn cause(&self) -> Option<&dyn Fail> {
        self.0.cause()
    }
    fn backtrace(&self) -> Option<&Backtrace> {
        self.0.backtrace()
    }
}
impl From<Error> for ErrorAsFail {
    fn from(e: Error) -> Self {
        ErrorAsFail(e)
    }
}
impl fmt::Debug for ErrorAsFail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}
impl fmt::Display for ErrorAsFail {
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
    impl <T: Fail> BoxFail for T {
        default fn set_cause(self, err: &mut Error) {
            err.0.cause = Some(Box::new(self));
        }
    }
    impl BoxFail for ErrorAsFail {
        fn set_cause(self, err: &mut Error) {
            if (self.0).0.cause.is_some() {
                err.0.cause = Some(Box::new(self));
            } else if let ErrorKind::GenericFail(_) = &(self.0).0.kind {
                match (self.0).0.kind {
                    ErrorKind::GenericFail(e) => {
                        err.0.cause = Some(e);
                        if err.0.backtrace.is_none() {
                            err.0.backtrace = (self.0).0.backtrace;
                        }
                    }
                    _ => unreachable!(),
                }
            } else {
                err.0.cause = Some(Box::new(self));
            }
        }
    }

    pub trait ToError {
        fn into_sylphie_error(self) -> Error;
    }
    impl <T: Fail> ToError for T {
        default fn into_sylphie_error(self) -> Error {
            Error::new(ErrorKind::GenericFail(Box::new(self)))
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
                Err(map(Error::new_with_backtrace(kind())))
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
                let cause = e.into_sylphie_error().with_backtrace().into_fail();
                Err(map(Error::new_with_cause(kind(), cause)))
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
macro_rules! cmd_error {
    ($format:expr, $($arg:expr),* $(,)?) => {{
        let text = format!($format, $($arg,)*);
        let err = crate::errors::Error::new(crate::errors::ErrorKind::CommandError(text.into()));
        return Err(err.into());
    }};
    ($str:expr) => {{
        let err = crate::errors::Error::new(crate::errors::ErrorKind::CommandError($str.into()));
        return Err(err.into());
    }};
}

/// Immediately returns with an internal error. The function must return an [`Error`], or a type
/// that it can be converted to using [`Into`].
#[macro_export]
macro_rules! bail {
    ($format:expr, $($arg:expr),* $(,)?) => {
        return Err(crate::errors::Error::new_with_backtrace(
            crate::errors::ErrorKind::InternalError(format!($format, $($arg,)*).into()),
        ).into())
    };
    ($str:expr) => {
        return Err(crate::errors::Error::new_with_backtrace(
            crate::errors::ErrorKind::InternalError($str.into()),
        ).into())
    };
}

/// Immediately returns with an internal error if a condition is `false`. The function must
/// return an [`Error`], or a type that it can be converted to using [`Into`].
#[macro_export]
macro_rules! ensure {
    ($check:expr, $($tt:tt)*) => {
        if !$check {
            bail!($($tt)*);
        }
    }
}


