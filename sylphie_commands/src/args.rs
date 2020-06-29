use crate::commands::Command;
use crate::ctx::{CommandArg, CommandCtx};
use static_events::prelude_async::*;
use sylphie_core::errors::*;

/// A helper type for parsing the arguments to command functions.
pub struct ArgsParserCtx<'a, E: Events> {
    ctx: &'a CommandCtx<E>,
    cmd: Command,
    current_idx: usize,
}
impl <'a, E: Events> ArgsParserCtx<'a, E> {
    pub fn new(ctx: &'a CommandCtx<E>, cmd: Command) -> Self {
        ArgsParserCtx {
            ctx,
            cmd,
            current_idx: 0,
        }
    }

    /// Returns the underlying command context.
    pub fn ctx(&self) -> &'a CommandCtx<E> {
        self.ctx
    }

    /// Returns the index of the current argument. This may be past the total number of arguments
    /// in the command context.
    pub fn current_arg(&self) -> usize {
        self.current_idx
    }

    /// Returns the current argument and increments the current argument.
    pub fn next_arg_raw(&mut self) -> Result<CommandArg<'a>> {
        if self.current_idx > self.ctx.args_count() {
            cmd_error!("Not enough arguments for command!");
            // TODO: give a better error output
        }

        let arg = self.ctx.arg(self.current_idx);
        self.current_idx += 1;
        Ok(arg)
    }

    pub fn next_arg<T: ParseArg<'a, E>>(&mut self) -> Result<T> {
        T::produce(self)
    }
}

/// A type that can be passed into a command function from its arguments.
///
/// Note that not all implementations of this trait produce values from the command arguments,
/// and may instead find them from other sources.
pub trait ParseArg<'a, E: Events> : Sized {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Result<Self>;
}

// Some basic "virtual" parameter types.
impl <'a, E: Events> ParseArg<'a, E> for &'a CommandCtx<E> {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Result<Self> {
        Ok(producer.ctx())
    }
}
impl <'a, E: Events> ParseArg<'a, E> for &'a Handler<E> {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Result<Self> {
        Ok(producer.ctx().handler())
    }
}
impl <'a, E: Events> ParseArg<'a, E> for Command {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Result<Self> {
        Ok(producer.cmd.clone())
    }
}

// Basic command parameter types.
impl <'a, E: Events> ParseArg<'a, E> for CommandArg<'a> {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Result<Self> {
        producer.next_arg_raw()
    }
}
impl <'a, E: Events> ParseArg<'a, E> for String {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Result<Self> {
        Ok(producer.next_arg_raw()?.text.to_string())
    }
}
impl <'a, E: Events> ParseArg<'a, E> for &'a str {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Result<Self> {
        Ok(producer.next_arg_raw()?.text)
    }
}
