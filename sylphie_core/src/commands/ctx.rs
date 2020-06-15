use crate::commands::args::*;
use crate::commands::commands::Command;
use crate::errors::*;
use futures::*;
use static_events::*;
use std::any::Any;

/// The implementation of a command context.
pub trait CommandCtxImpl: Send + 'static {
    /// Hints to the command context the command that is being executed.
    ///
    /// This is primarily meant for reporting better errors to the user, and is automatically
    /// called by [`Command::execute`].
    fn set_command_hint(&mut self, _command: &Command) { }

    /// Controls the way the arguments to commands in this context are parsed.
    fn args_parsing_options(&self, _target: &Handler<impl Events>) -> ArgParsingOptions {
        ArgParsingOptions::default()
    }

    /// Returns the raw message string to parse as a commmand.
    ///
    /// This should return the same value for every call.
    fn raw_message(&self) -> &str;
}

/// An interface containing all functions common to a [`CommandCtx`] and unerased command
/// contexts.
///
/// This is meant as a utility for working with modules that provide a wrapper over commmand
/// contexts that expose context-specific functions.
pub trait CommandCtxInterface<E: Events> {
    // TODO
}

/// The context for a given command.
pub struct CommandCtx<E: Events> {
    handle: Handler<E>,
    ctx_impl: Box<dyn CommandCtxImplWrapper<E>>,
}
impl <E: Events> CommandCtx<E> {
    /// Creates a new command context given an implementation and a [`Handler`].
    pub fn new(target: &Handler<E>, ctx_impl: impl CommandCtxImpl) -> Self {
        CommandCtx {
            handle: target.clone(),
            ctx_impl: Box::new(ctx_impl),
        }
    }

    /// Attempts to downcasts the internal [`CommandCtxImpl`] to a reference to the given type.
    ///
    /// This is not generally useful and should usually be wrapped by a context-specific helper.
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.ctx_impl.as_any().downcast_ref::<T>()
    }

    /// Attempts to downcasts the internal [`CommandCtxImpl`] to a mutable reference to the
    /// given type.
    ///
    /// This is not generally useful and should usually be wrapped by a context-specific helper.
    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.ctx_impl.as_any_mut().downcast_mut::<T>()
    }

    /// Pushes a command hint.
    pub(in super) fn set_command_hint(&mut self, command: &Command) {
        self.ctx_impl.set_command_hint(command);
    }

    /// Returns the raw text of the command.
    fn raw_message(&self) -> &str {
        self.ctx_impl.raw_message()
    }
}


/// An object-safe wrapper around [`CommandCtxImpl`].
trait CommandCtxImplWrapper<E: Events>: Send + 'static {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn set_command_hint(&mut self, command: &Command);
    fn raw_message(&self) -> &str;
}
impl <E: Events, T: CommandCtxImpl> CommandCtxImplWrapper<E> for T {
    fn as_any(&self) -> &dyn Any { self }
    fn as_any_mut(&mut self) -> &mut dyn Any { self }
    fn set_command_hint(&mut self, command: &Command) { self.set_command_hint(command); }
    fn raw_message(&self) -> &str { self.raw_message() }
}