#![feature(specialization, type_alias_impl_trait, marker_trait_attr, trivial_bounds)]
#![deny(unused_must_use)]

#[macro_use] extern crate tracing;

use static_events::*;
use std::path::PathBuf;

#[macro_use] pub mod errors;

mod core;
pub mod interface;
pub mod module;

pub use crate::core::*;

/// Reexports of various types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_export {
    pub use enumset::EnumSet;
    pub use static_events;
    pub use std::prelude::v1::{Default, Some, None};
}