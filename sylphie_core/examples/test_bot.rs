use static_events::*;
use sylphie_core::prelude::*;
use sylphie_core::interface::TerminalCommandEvent;

#[derive(Module)]
pub struct Test {
    #[module_info] info: ModuleInfo,
    #[core_ref] core: CoreRef<MyModule>,
}

#[derive(Module)]
#[module(integral_recursive)]
pub struct MyModule {
    #[module_info] info: ModuleInfo,
    #[submodule] test: Test,
    #[init_with { 3 + 3 }] foo: u32,
}

fn failable() -> Result<()> {
    bail!("!!!");
}

#[events_impl]
impl MyModule {
    #[event_handler]
    fn terminal_error_test(&self, ev: &TerminalCommandEvent) -> EventResult {
        if &ev.0 == "!test" {
            if let Err(err) = failable() {
                err.report_error();
            }
            EvCancel
        } else {
            EvOk
        }
    }
}

fn main() {
    SylphieCore::<MyModule>::new("test_bot").start().unwrap();
}
