#![feature(specialization)]
#![deny(unused_must_use)]

// TODO: Remove the minnie_errors dependency and add a mechanism to hook error reports.

#[macro_use] extern crate tracing;
pub mod errors; // this goes before to make sure macros resolve

pub mod core;
pub mod interface;
pub mod module;
pub mod timer;
pub mod utils;

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
    /// The phase for `#[module_impl]`.
    pub enum ModuleImplPhase { }

    // Helps statically check that `#[module(component)]` modules are properly registered.
    pub trait IsComponent { }
    pub struct ThisTypeMustBeUsedAsASubmodule;
    pub trait CheckIsComponent<T> {
        fn item(_: ThisTypeMustBeUsedAsASubmodule) { todo!() }
    }
    impl <T> CheckIsComponent<u32> for T { }
    impl <T: IsComponent> CheckIsComponent<u64> for T { }
}

/// A convenience module containing common imports that are useful throughout Sylphie-based code.
pub mod prelude {
    pub use crate::core::{SylphieCore, SylphieCoreHandlerExt};
    pub use crate::errors::{Error, ErrorKind, ErrorWrapper, ErrorFromContextExt, Result};
    pub use crate::errors::{cmd_error, bail, ensure};
    pub use crate::module::{Module, ModuleInfo};
    pub use static_events::prelude_async::*;
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
