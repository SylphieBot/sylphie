pub use sylphie_core::core;
pub use sylphie_core::database;
pub use sylphie_core::errors;
pub use sylphie_core::interface;
pub use sylphie_core::module;

pub mod commands {
    pub use sylphie_commands::{commands, ctx, manager};
}

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

            #[submodule]
            commands: $crate::__macro_export::sylphie_commands::CommandsModule<$mod_name>,
            $(
                #[submodule] $(#[$meta])*
                $name: $ty,
            )*
            #[submodule]
            __sylphie_marker__: $crate::__macro_export::WrapperModule,
        }
    };
}

/// Reexports of various types for macros. Not public API.
#[doc(hidden)]
pub mod __macro_export {
    pub use sylphie_commands;
    pub use sylphie_core;

    use sylphie_derive::CoreModule;
    #[derive(CoreModule)]
    #[module(integral, anonymous)]
    pub struct WrapperModule {
        #[module_info] info: crate::module::ModuleInfo,
    }
}

/// A convenience module containing common imports that are useful throughout Sylphie-based code.
pub mod prelude {
    pub use crate::derives::*;
    pub use crate::sylphie_root_module;
    pub use sylphie_commands::prelude::*;
    pub use sylphie_core::prelude::*;
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