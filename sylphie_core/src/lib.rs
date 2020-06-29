#![feature(specialization)]
#![deny(unused_must_use)]

// TODO: Properly scope our macros using the local use trick.
// TODO: Remove the minnie_errors dependency and add a mechanism to hook error reports.

#[macro_use] extern crate tracing;
#[macro_use] pub mod errors;

pub mod commands;
pub mod core;
pub mod database;
pub mod interface;
pub mod module;
mod utils;

pub use crate::core::SylphieCore;
pub use crate::errors::{Result, Error};

/// Reexports of various types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_export {
    pub use enumset::EnumSet;
    pub use static_events;
    pub use std::prelude::v1::{Option, Default, Some, None, Ok, Err};
}

/// Various utility functions and types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_priv {
    use crate::prelude::*;
    use std::any::TypeId;

    /// Converts a `CoreRef` of one type to another. Avoids generics weirdness.
    pub fn cast_core_ref<M: Module, M2: Module>(core: CoreRef<M>) -> CoreRef<M2> {
        if TypeId::of::<CoreRef<M>>() == TypeId::of::<CoreRef<M2>>() {
            unsafe { std::mem::transmute(core) }
        } else {
            panic!("Could not cast CoreRef. Check if the types are correct.");
        }
    }

    /// The phase for `#[module_impl]`.
    pub enum ModuleImplPhase { }
}

/// A convenience module containing common imports that are useful throughout Sylphie-based code.
pub mod prelude {
    pub use crate::{cmd_error, bail, ensure};
    pub use crate::core::{SylphieCore, SylphieHandlerExt, CoreRef};
    pub use crate::derives::*;
    pub use crate::errors::{Error, ErrorKind, ErrorFromContextExt, Result};
    pub use crate::module::{Module, ModuleInfo};
    pub use static_events::prelude_async::{Handler, Events};
    pub use static_events::prelude_async::{EventResult, EvOk, EvCancel, EvCancelStage};
    pub use static_events::prelude_async::{EvCheck, EvInit, EvBeforeEvent, EvOnEvent, EvAfterEvent};
    pub use std::result::{Result as StdResult};
}

/// Exports the derives used for this crate.
pub mod derives {
    #[doc(inline)] pub use sylphie_derive::{
        CoreModule as Module,
        module_impl_core as module_impl,
        command,
    };
    #[doc(inline)] pub use static_events::handlers::event_handler;
}
