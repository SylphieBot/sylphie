use crate::errors::*;
use crate::module::*;
use futures::*;
use static_events::*;
use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

mod args;
pub mod commands;
pub mod ctx;

pub use commands::Command;
pub use ctx::CommandCtx;

/// The event used to register commands.
#[derive(Debug, Default)]
pub struct RegisterCommandsEvent {
    commands: Vec<Command>,
}
self_event!(Command);
impl RegisterCommandsEvent {
    /// Registers a new command.
    pub fn register_command(&mut self, command: Command) {
        self.commands.push(command);
    }
}

/// The service used to lookup commands.
#[derive(Debug)]
pub struct CommandManager {
    commands: Vec<Command>,
}
impl CommandManager {
}
