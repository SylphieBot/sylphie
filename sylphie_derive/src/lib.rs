#![feature(proc_macro_diagnostic, proc_macro_span, drain_filter)]
#![recursion_limit="256"]

extern crate proc_macro;

use proc_macro::TokenStream;
use static_events_internals::*;

mod derive;

// Note that we explicitly handle any attributes that are part of Events.
#[proc_macro_derive(Module, attributes(
    module, submodule, subhandler, service, module_info, init_with, core_ref,
))]
pub fn derive_module(input: TokenStream) -> TokenStream {
    try_syn!(derive::derive_events(input))
}
