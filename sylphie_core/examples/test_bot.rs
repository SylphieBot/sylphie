use futures::*;
use futures::future::BoxFuture;
use static_events::*;
use sylphie_core::commands::commands::*;
use sylphie_core::commands::manager::*;
use sylphie_core::prelude::*;
use sylphie_core::commands::ctx::CommandCtx;

struct MyCommandImpl;
impl CommandImpl for MyCommandImpl {
    fn execute<'a>(&'a self, ctx: &'a CommandCtx<impl SyncEvents>) -> BoxFuture<'a, Result<()>> {
        async move {
            for arg in 0..ctx.args_count() {
                ctx.respond(&format!("Arg #{}: {:?}", arg, ctx.arg(arg).text)).await?;
            }
            Ok(())
        }.boxed()
    }
}

struct BacktraceCommandImpl;
impl CommandImpl for BacktraceCommandImpl {
    fn execute<'a>(&'a self, ctx: &'a CommandCtx<impl SyncEvents>) -> BoxFuture<'a, Result<()>> {
        async move {
            ctx.respond(&format!("\n\n{:?}", backtrace::Backtrace::new())).await?;
            Ok(())
        }.boxed()
    }
}

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
    fn add_command(&self, target: &Handler<impl Events>, ev: &mut RegisterCommandsEvent) {
        // TODO: Temporary before we get a proper procedural macro in here.
        ev.register_command(Command::new_dynamic(
            target, "test.module.foo", CommandInfo::new("test"), MyCommandImpl,
        ));
        ev.register_command(Command::new_dynamic(
            target, "test.module.foo", CommandInfo::new("backtrace"), BacktraceCommandImpl,
        ));
    }
}

fn main() {
    SylphieCore::<MyModule>::new("test_bot").start().unwrap();
}
