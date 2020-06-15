use crate::commands::ctx::{CommandArg, CommandCtx};
use static_events::*;

/// A helper type for parsing the arguments to command functions.
pub struct ArgsParserCtx<'a, E: Events> {
    ctx: &'a CommandCtx<E>,
    current_idx: usize,
}
impl <'a, E: Events> ArgsParserCtx<'a, E> {
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
    pub fn next_arg(&mut self) -> CommandArg<'a> {
        let arg = self.ctx.arg(self.current_idx);
        self.current_idx += 1;
        arg
    }
}

/// A type that can be passed into a command function from its arguments.
///
/// Note that not all implementations of this trait produce values from the command arguments,
/// and may instead find them from other sources.
pub trait ParseArg<'a, E: Events> {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Self;
}

// Some basic "virtual" parameter types.
impl <'a, E: Events> ParseArg<'a, E> for CommandArg<'a> {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Self {
        producer.next_arg()
    }
}
impl <'a, E: Events> ParseArg<'a, E> for &'a CommandCtx<E> {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Self {
        producer.ctx()
    }
}
impl <'a, E: Events> ParseArg<'a, E> for &'a Handler<E> {
    fn produce(producer: &mut ArgsParserCtx<'a, E>) -> Self {
        producer.ctx().handler()
    }
}

// Concrete parameter types.
// TODO