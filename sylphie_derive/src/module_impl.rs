use crate::CratePaths;
use darling::*;
use proc_macro::TokenStream;
use static_events_internals::{*, Error, Result};
use static_events_internals::utils::*;
use syn::*;
use syn::spanned::Spanned;
use quote::*;

#[derive(FromMeta, Debug, Default)]
struct CommandAttrs {
    #[darling(default)]
    name: Option<String>,
}

#[derive(Debug)]
enum HandlerType {
    Command(CommandAttrs),
}
impl HandlerType {
    fn is_attr(attr: &Attribute) -> bool {
        match last_path_segment(&attr.path).as_str() {
            "command" => true,
            _ => false,
        }
    }
    fn for_attr(attr: &Attribute) -> Result<Option<HandlerType>> {
        match last_path_segment(&attr.path).as_str() {
            "command" => {
                let meta = if attr.tokens.is_empty() {
                    CommandAttrs::default()
                } else {
                    FromMeta::from_meta(&attr.parse_meta()?)?
                };
                Ok(Some(HandlerType::Command(meta)))
            },
            _ => Ok(None),
        }
    }
    fn for_method(method: &ImplItemMethod) -> Result<Option<HandlerType>> {
        let mut handler_type: Option<HandlerType> = None;
        for attr in &method.attrs {
            if let Some(tp) = HandlerType::for_attr(attr)? {
                if let Some(e_tp) = &handler_type {
                    error(
                        attr.span(),
                        if e_tp.name() == tp.name() {
                            format!("{} can only be used once.", tp.name())
                        } else {
                            format!("{} cannot be used with {}.", tp.name(), e_tp.name())
                        }
                    )?;
                }
                handler_type = Some(tp);
            }
        }
        Ok(handler_type)
    }
    fn name(&self) -> &'static str {
        match self {
            HandlerType::Command(_) => "#[command]",
        }
    }
}

fn mark_attrs_processed(method: &mut ImplItemMethod) {
    for attr in &mut method.attrs {
        if HandlerType::is_attr(attr) {
            mark_attribute_processed(attr);
        }
    }
}

fn create_command_handler(
    paths: &CratePaths, events: &mut EventsImplAttr, attrs: &CommandAttrs, method: &ImplItemMethod,
) -> Result<()> {
    let core = &paths.core;
    let commands = &paths.commands;
    let static_events = quote! { #core::__macro_export::static_events };

    if !method.sig.generics.params.is_empty() {
        return Err(Error::new(
            method.sig.generics.span(), "#[command] methods may not be generic.",
        ));
    }

    let name_str = method.sig.ident.to_string();
    let cmd_name = attrs.name.as_ref().map(|x| &**x).unwrap_or_else(|| {
        if name_str.starts_with("cmd_") {
            &name_str[4..]
        } else {
            &name_str
        }
    });
    let command_info = quote! { #commands::commands::CommandInfo::new(#cmd_name) };

    // TODO: Assert the command is async correctly.
    // TODO: Support commands without a self parameter.
    let ev_call = &method.sig.ident;
    let mut ev_call_params = Vec::new();
    for _ in 1..method.sig.inputs.len() {
        ev_call_params.push(quote! { _ctx.next_arg()? })
    }

    let cmd_marker = ident!("ModuleImpl_CommandMarker_{}", ev_call);
    let cmd_impl = ident!("__module_impl__impl_{}", ev_call);
    let execute_cmd = ident!("__module_impl__execute_{}", ev_call);
    let register_cmd = ident!("__module_impl__register_{}", ev_call);
    events.add_extra_item(quote! {
        enum #cmd_marker { }
    });
    events.process_synthetic_method(quote! {
        async fn #cmd_impl(
            &self, mut _ctx: #commands::args::ArgsParserCtx<'_, impl #static_events::Events>,
        ) -> #core::errors::Result<()> {
            self.#ev_call(#(#ev_call_params,)*).await
        }
    })?;
    events.process_synthetic_method(quote! {
        #[#static_events::event_handler]
        async fn #execute_cmd<E: #static_events::Events>(
            &self,
            ev: &#commands::__macro_priv::ExecuteCommand<#cmd_marker, E>,
            state: &mut #core::__macro_export::Option<#core::errors::Result<()>>,
        ) {
            if ev.mod_id == #core::module::Module::info(self).id() {
                if state.is_some() {
                    #commands::__macro_priv::duplicate_module_id();
                }
                let parser_ctx = #commands::args::ArgsParserCtx::new(&ev.ctx, ev.cmd.clone());
                *state = #core::__macro_export::Some(self.#cmd_impl(parser_ctx).await);
            }
        }
    })?;
    events.process_synthetic_method(quote! {
        #[#static_events::event_handler]
        fn #register_cmd(
            &self,
            target: &#static_events::Handler<impl #static_events::Events>,
            ev: &mut #commands::manager::RegisterCommandsEvent,
        ) {
            struct CommandImpl(#core::module::ModuleId);
            impl #commands::commands::CommandImpl for CommandImpl {
                fn execute<'a>(
                    &'a self,
                    cmd: #commands::commands::Command,
                    ctx: &'a #commands::ctx::CommandCtx<impl #static_events::Events>,
                ) -> #commands::__macro_export::BoxFuture<'a, #core::errors::Result<()>> {
                    #commands::__macro_export::FutureExt::boxed(async move {
                        match ctx.handler().dispatch_async(
                            #commands::__macro_priv::ExecuteCommand::<#cmd_marker, _>::new(
                                self.0, cmd.clone(), ctx.clone(),
                            ),
                        ).await {
                            #core::__macro_export::Some(v) => v,
                            #core::__macro_export::None =>
                                #commands::__macro_priv::module_not_found(),
                        }
                    })
                }
            }

            let id = #core::module::Module::info(self).id();
            ev.register_command(#commands::commands::Command::new(
                target, self, #command_info, CommandImpl(id),
            ));
        }
    })?;
    Ok(())
}
fn process_items(
    paths: &CratePaths, events: &mut EventsImplAttr, input: &mut ItemImpl,
) -> Result<()> {
    let mut errors = Error::empty();
    for item in &mut input.items {
        match item {
            ImplItem::Method(method) => {
                match HandlerType::for_method(method) {
                    Ok(Some(HandlerType::Command(cmd))) =>
                        if let Err(e) = create_command_handler(paths, events, &cmd, method) {
                            errors = errors.combine(e);
                        },
                    Ok(None) => { }
                    Err(e) => errors = errors.combine(e),
                }
                mark_attrs_processed(method);
            },
            _ => { }
        }
    }
    if !errors.is_empty() {
        Err(errors)
    } else {
        Ok(())
    }
}

pub(crate) fn derive_impl(paths: &CratePaths, input: TokenStream) -> Result<TokenStream> {
    let mut input: ItemImpl = parse(input)?;

    let mut events = EventsImplAttr::new(
        &mut input,
        Some(quote! { ::sylphie_core::__macro_export::static_events }),
    )?;
    events.set_discriminator(quote! { ::sylphie_core::__macro_priv::ModuleImplPhase });
    process_items(paths, &mut events, &mut input)?;
    let events_impl = events.generate();

    Ok((quote! {
        #input
        #events_impl
    }).into())
}
