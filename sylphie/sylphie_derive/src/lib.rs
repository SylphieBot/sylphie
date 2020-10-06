#![recursion_limit="256"]

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::{TokenStream as SynTokenStream};
use static_events_internals::*;
use quote::*;

mod derive;
mod module_impl;

pub(crate) struct CratePaths {
    core: SynTokenStream,
    commands: SynTokenStream,
    database: SynTokenStream,
}
fn crate_paths_for_sylphie() -> CratePaths {
    CratePaths {
        core: quote! { ::sylphie::__macro_export::sylphie_core },
        commands: quote! { ::sylphie::__macro_export::sylphie_commands },
        database: quote! { ::sylphie::__macro_export::sylphie_database },
    }
}
fn crate_paths_for_core() -> CratePaths {
    CratePaths {
        core: quote! { ::sylphie_core },
        commands: quote! { ::sylphie_commands },
        database: quote! { ::sylphie_database },
    }
}
fn crate_paths_for_core_internal() -> CratePaths {
    CratePaths {
        core: quote! { crate },
        commands: quote! { __CANNOT_USE_COMMANDS_IN_CORE_INTERNAL__ },
        database: quote! { __CANNOT_USE_COMMANDS_IN_CORE_INTERNAL__ },
    }
}

// Note that we explicitly handle any attributes that are part of Events.
#[proc_macro_derive(SylphieModule, attributes(
    module, submodule, subhandler, service, module_info, init_with,
))]
pub fn derive_module_sylphie(input: TokenStream) -> TokenStream {
    try_syn!(derive::derive_events(&crate_paths_for_sylphie(), input))
}
#[proc_macro_derive(CoreModule, attributes(
    module, submodule, subhandler, service, module_info, init_with,
))]
pub fn derive_module_core(input: TokenStream) -> TokenStream {
    try_syn!(derive::derive_events(&crate_paths_for_core(), input))
}
#[proc_macro_derive(CoreInternalModule, attributes(
    module, submodule, subhandler, service, module_info, init_with,
))]
pub fn derive_module_core_internal(input: TokenStream) -> TokenStream {
    try_syn!(derive::derive_events(&crate_paths_for_core_internal(), input))
}

#[proc_macro_attribute]
pub fn module_impl_sylphie(_: TokenStream, item: TokenStream) -> TokenStream {
    try_syn!(module_impl::derive_impl(&crate_paths_for_sylphie(), item))
}
#[proc_macro_attribute]
pub fn module_impl_core(_: TokenStream, item: TokenStream) -> TokenStream {
    try_syn!(module_impl::derive_impl(&crate_paths_for_core(), item))
}

derived_attr!(command, module_impl);
derived_attr!(config, module_impl);
