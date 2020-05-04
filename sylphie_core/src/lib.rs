#![feature(specialization, type_alias_impl_trait, marker_trait_attr, trivial_bounds)]
#![deny(unused_must_use)]

#[macro_use] extern crate tracing;
#[macro_use] pub mod errors;

pub mod core;
pub mod database;
pub mod interface;
pub mod module;

pub use crate::core::SylphieCore;
pub use crate::errors::{Result, Error};

/// Reexports of various types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_export {
    pub use enumset::EnumSet;
    pub use static_events;
    pub use std::prelude::v1::{Default, Some, None};
}

/// Various utility functions and types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_priv {
    use crate::prelude::*;
    use std::any::TypeId;

    pub fn cast_sylphie_core<M: Module, M2: Module>(core: SylphieCore<M>) -> SylphieCore<M2> {
        if TypeId::of::<SylphieCore<M>>() == TypeId::of::<SylphieCore<M2>>() {
            unsafe { std::mem::transmute(core) }
        } else {
            panic!("Could not cast SylphieCore. Check if the types are correct.");
        }
    }
}

/// A convinence module containing common imports that are useful throughout Sylphie-based code.
pub mod prelude {
    pub use crate::core::{SylphieCore, SylphieHandlerExt};
    pub use crate::errors::{Error, ErrorKind, ErrorFromContextExt, Result};
    pub use crate::module::{Module, ModuleInfo};
    pub use std::result::{Result as StdResult};
}