use static_events::prelude_async::*;
use sylphie::database::kvs::*;
use sylphie::prelude::*;

#[derive(Module)]
#[module(integral_recursive)]
pub struct TestModule {
    #[module_info] info: ModuleInfo,
    #[submodule] test_store_1: KvsStore<u32, f32>,
    #[submodule] test_store_2: TransientKvsStore<u32, f32>,
}
#[module_impl]
impl TestModule {
    #[command]
    async fn cmd_test_mod(&self, ctx: &CommandCtx<impl Events>) -> Result<()> {
        ctx.respond(&format!("Calling module: {}", self.info.name())).await?;
        Ok(())
    }
}

#[derive(Module)]
#[module(integral_recursive)]
pub struct MyModule {
    #[module_info] info: ModuleInfo,
    #[submodule] submod_a: TestModule,
    #[submodule] submod_b: TestModule,
    #[submodule] kvs: KvsStore<String, String>,
}

#[module_impl]
impl MyModule {
    #[command]
    async fn cmd_test(&self, ctx: &CommandCtx<impl Events>) -> Result<()> {
        for (i, scope) in ctx.scopes().iter().enumerate() {
            ctx.respond(&format!("Scope #{}: {:?}", i, scope)).await?;
        }
        for arg in 0..ctx.args_count() {
            ctx.respond(&format!("Arg #{}: {:?}", arg, ctx.arg(arg).text)).await?;
        }
        Ok(())
    }

    #[command]
    async fn cmd_backtrace(&self, ctx: &CommandCtx<impl Events>) -> Result<()> {
        ctx.respond(&format!("\n\n{:?}", backtrace::Backtrace::new())).await?;
        Ok(())
    }

    #[command]
    async fn cmd_test_panic(&self) -> Result<()> {
        panic!("User requested panic.")
    }

    #[command]
    async fn cmd_test_error(&self) -> Result<()> {
        bail!("User requested error.")
    }

    #[command]
    async fn cmd_kvs_set(
        &self, ctx: &CommandCtx<impl Events>, key: String, val: String,
    ) -> Result<()> {
        let cur = self.kvs.get(key.clone()).await?;
        ctx.respond(&format!("Current value for {}: {:?}", key, cur)).await?;
        self.kvs.set(key.clone(), val.clone()).await?;
        ctx.respond(&format!("New     value for {}: {:?}", key, val)).await?;
        Ok(())
    }

    #[command]
    async fn cmd_kvs_append(
        &self, ctx: &CommandCtx<impl Events>, key: String, val: String,
    ) -> Result<()> {
        let mut lock = self.kvs.get_mut_default(key.clone()).await?;
        ctx.respond(&format!("Current value for {}: {:?}", key, &*lock)).await?;
        lock.push_str(&val);
        ctx.respond(&format!("New     value for {}: {:?}", key, &*lock)).await?;
        lock.commit().await?;
        Ok(())
    }
}

sylphie_root_module! {
    module Test {
        test_bot: MyModule,
        core: sylphie_mod_core::ModCore,
    }
}

fn main() {
    SylphieCore::<Test>::new("test_bot").start().unwrap();
}
