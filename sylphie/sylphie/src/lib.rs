//! A generic bot framework designed for allowing bot components to be cleanly and modularly
//! combined.

#[doc(inline)] pub use sylphie_core::core;
#[doc(inline)] pub use sylphie_core::errors;
#[doc(inline)] pub use sylphie_core::interface;
#[doc(inline)] pub use sylphie_core::timer;
#[doc(inline)] pub use sylphie_core::module;

/// A module containing the implementaiton of Sylphie's commands system.
pub mod commands {
    #[doc(inline)] pub use sylphie_commands::{commands, ctx, manager};
}

/// A module containing the implementation of Sylphie's database system.
pub mod database {
    #[doc(inline)] pub use sylphie_database::{connection, config, kvs, migrations, serializable};
}

/// A module containing various types useful for the construction of Sylphie bots.
pub mod utils {
    #[doc(inline)] pub use sylphie_utils::{cache, disambiguate, locks};

    /// Types used to specify particular contexts such as users, members or servers.
    pub mod scopes {
        #[doc(inline)] pub use sylphie_database::utils::ScopeId;
        #[doc(inline)] pub use sylphie_utils::scopes::*;
    }

    /// Types for working with strings efficiently easier.
    pub mod strings {
        #[doc(inline)] pub use sylphie_database::utils::StringId;
        #[doc(inline)] pub use sylphie_utils::strings::*;
    }
}

/// A macro for constructing a Sylphie root module with all the standard submodules attached.
///
/// It is recommended that you use this rather than manually creating the root module.
#[macro_export]
macro_rules! sylphie_root_module {
    (
        module $mod_name:ident {$(
            $(#[$meta:meta])*
            $name:ident: $ty:ty,
        )*}
    ) => {
        #[derive($crate::derives::Module)]
        #[module(integral)]
        pub struct $mod_name {
            #[module_info] info: $crate::module::ModuleInfo,
            $(
                #[submodule] $(#[$meta])*
                $name: $ty,
            )*
            #[submodule] core: $crate::__macro_export::CoreModule<$mod_name>,
        }
    };
}

pub use sylphie_core::core::SylphieCore;

/// Reexports of various types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_export {
    pub use sylphie_commands;
    pub use sylphie_core;
    pub use sylphie_database;

    #[derive(sylphie_derive::CoreModule)]
    #[module(integral)]
    pub struct CoreModule<M: crate::module::Module> {
        #[module_info]
        info: crate::module::ModuleInfo,
        #[submodule]
        commands: sylphie_commands::CommandsModule<M>,
        #[subhandler]
        #[init_with { sylphie_database::DatabaseModule::new() }]
        database: sylphie_database::DatabaseModule,
    }
}

/// A convenience module containing common imports that are useful throughout Sylphie-based code.
pub mod prelude {
    pub use crate::derives::*;
    pub use crate::sylphie_root_module;
    pub use sylphie_commands::prelude::*;
    pub use sylphie_core::prelude::*;
    pub use sylphie_utils::scopes::{Scope, ScopeArgs};
    pub use sylphie_utils::strings::StringWrapper;
}

/// Exports the derives used for this crate.
pub mod derives {
    #[doc(inline)] pub use sylphie_derive::{
        SylphieModule as Module,
        module_impl_sylphie as module_impl,
        command,
    };
    #[doc(inline)] pub use static_events::handlers::event_handler;
}