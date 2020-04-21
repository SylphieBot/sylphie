use darling::*;
use git2::{*, Error as GitError};
use proc_macro::TokenStream;
use proc_macro2::{TokenStream as SynTokenStream};
use static_events_internals::{*, Result};
use static_events_internals::utils::*;
use syn::*;
use syn::spanned::Spanned;
use quote::*;

#[derive(Default)]
struct FieldAttrs {
    is_module_info: bool,
    is_submodule: bool,
}
impl FieldAttrs {
    fn from_attrs(attrs: &[Attribute]) -> FieldAttrs {
        let mut tp = FieldAttrs::default();
        for attr in attrs {
            match last_path_segment(&attr.path).as_str() {
                "module_info" => tp.is_module_info = true,
                "submodule" => tp.is_submodule = true,
                _ => { }
            }
        }
        tp
    }
}

#[derive(FromDeriveInput)]
#[darling(attributes(module))]
struct ModuleAttrs {
    #[darling(default)]
    integral: bool,
    #[darling(default)]
    integral_recursive: bool,
}

fn git_metadata() -> std::result::Result<SynTokenStream, GitError> {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(v) => v,
        _ => return Err(GitError::from_str("env error")),
    };
    let repo: Repository = Repository::discover(manifest_dir)?;

    let head = repo.head()?;

    let revision = head.peel_to_commit()?.id().to_string();
    let name = head.shorthand().unwrap_or(&revision);
    let changed_files = repo.diff_tree_to_workdir(Some(&head.peel_to_tree()?), None)?.deltas()
        .filter(|x| x.status() != Delta::Unmodified)
        .count() as u32;

    Ok(quote! {
        ::sylphie_core::module::GitInfo {
            name: #name,
            revision: #revision,
            modified_files: #changed_files,
        }
    })
}
fn module_metadata(attrs: &ModuleAttrs) -> SynTokenStream {
    let mut flags = SynTokenStream::new();
    if attrs.integral {
        flags.extend(quote! { | ::sylphie_core::module::ModuleFlag::Integral });
    }
    if attrs.integral_recursive {
        flags.extend(quote! { | ::sylphie_core::module::ModuleFlag::IntegralRecursive });
    }
    let git_info = match git_metadata() {
        Ok(v) => quote! { ::sylphie_core::__macro_export::Some(#v) },
        _ => quote! { ::sylphie_core::__macro_export::None },
    };
    quote! {
        ::sylphie_core::module::ModuleMetadata {
            module_path: ::std::module_path!(),
            crate_version: ::std::option_env!("CARGO_PKG_VERSION").unwrap_or("<unknown>"),
            git_info: #git_info,
            flags: ::sylphie_core::__macro_export::EnumSet::new() #flags,
        }
    }
}
fn derive_module(input: &mut DeriveInput) -> Result<SynTokenStream> {
    let attrs: ModuleAttrs = ModuleAttrs::from_derive_input(input)?;
    let input_span = input.span();
    let data = if let Data::Struct(data) = &mut input.data {
        data
    } else {
        error(input.span(), "#[derive(Module)] may only be used with structs.")?
    };
    if let Fields::Named(_) = data.fields {
        // ...
    } else {
        error(input_span, "#[derive(Module)] can only be used on structs with named fields.")?;
    }

    let metadata = module_metadata(&attrs);

    let ident = &input.ident;
    let impl_generics = &input.generics;
    let (bounds, ty_bounds, where_bounds) = impl_generics.split_for_impl();

    let mut field_names = Vec::new();
    let mut fields = Vec::new();
    let mut info_field = None;
    for field in &mut data.fields {
        let attrs = FieldAttrs::from_attrs(&field.attrs);

        if attrs.is_module_info {
            if info_field.is_some() {
                error(field.span(), "Only one #[module_info] field may be present.")?;
            }
            info_field = Some(&field.ident);
        }

        field_names.push(field.ident.clone().unwrap());
        if attrs.is_submodule {
            // Push a `#[submodule]` attribute to pass to static-events
            field.attrs.push(Attribute {
                pound_token: Default::default(),
                style: AttrStyle::Outer,
                bracket_token: Default::default(),
                path: parse2(quote!(subhandler))?,
                tokens: Default::default()
            });

            let name = &field.ident;
            fields.push(quote! { _walker.register_module(_parent, stringify!(#name)) });
        } else {
            fields.push(quote! { ::sylphie_core::__macro_export::Default::default() });
        }
    }
    let info_field = match info_field {
        Some(v) => v,
        _ => error(input_span, "At least one field must be marked with #[module_info].")?,
    };

    Ok(quote! {
        impl #bounds ::sylphie_core::module::Module for #ident #ty_bounds #where_bounds {
            fn metadata(&self) -> ::sylphie_core::module::ModuleMetadata {
                #metadata
            }

            fn info(&self) -> &::sylphie_core::module::ModuleInfo {
                &self.#info_field
            }
            fn info_mut(&mut self) -> &mut ::sylphie_core::module::ModuleInfo {
                &mut self.#info_field
            }

            fn init_module<R: ::sylphie_core::module::Module>(
                _parent: &str, _walker: &mut ::sylphie_core::module::ModuleTreeWalker<R>,
            ) -> Self {
                #ident {
                    #(#field_names: #fields,)*
                }
            }
        }
    })
}

pub fn derive_events(input: TokenStream) -> Result<TokenStream> {
    let mut input: DeriveInput = parse(input)?;

    let module_impl = match derive_module(&mut input) {
        Ok(v) => v,
        Err(e) => e.emit().into(),
    };
    let events = DeriveStaticEvents::new(
        &input, Some(quote! { ::sylphie_core::__macro_export::static_events }),
    )?;
    let events_impl = events.generate();

    Ok((quote! {
        const _: () = {
            #module_impl
            #events_impl
        };
    }).into())
}
