#[macro_use] extern crate tracing;

pub mod args;
pub mod commands;
pub mod ctx;
pub mod manager;
mod module;
mod raw_args;

pub use module::CommandsModule;

/// A convenience module containing common imports.
pub mod prelude {
    pub use crate::commands::{Command, CommandInfo};
    pub use crate::ctx::{CommandCtx, CommandArg};
}

/// Reexports of various types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_export {
    pub use futures::FutureExt;
    pub use futures::future::BoxFuture;
}

/// Various utility functions and types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_priv {
    use crate::commands::Command;
    use crate::ctx::CommandCtx;
    use static_events::prelude_async::*;
    use std::marker::PhantomData;
    use sylphie_core::*;
    use sylphie_core::module::ModuleId;

    pub struct ExecuteCommand<T, E: Events> {
        pub mod_id: ModuleId,
        pub cmd: Command,
        pub ctx: CommandCtx<E>,
        phantom: PhantomData<fn(T)>,
    }
    simple_event!([T, E: Events] ExecuteCommand<T, E>, Option<Result<()>>);
    impl <T, E: Events> ExecuteCommand<T, E> {
        pub fn new(id: ModuleId, cmd: Command, ctx: CommandCtx<E>) -> Self {
            ExecuteCommand {
                mod_id: id,
                cmd,
                ctx,
                phantom: PhantomData,
            }
        }
    }

    #[inline(never)] #[cold]
    pub fn duplicate_module_id() -> ! {
        panic!("Duplicate module ID!!")
    }
    #[inline(never)] #[cold]
    pub fn module_not_found() -> ! {
        panic!("Module not found for command!")
    }
}