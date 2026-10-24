//! Entry points for the two separately compiled halves of a mod.

use proc_macro::TokenStream;
use quote::quote;

/// Exports an implementation of `experience_sdk::server::Experience`.
#[proc_macro_attribute]
pub fn server(options: TokenStream, input: TokenStream) -> TokenStream {
    entry(options, input, true)
}

/// Exports an implementation of `experience_sdk::client::ClientPart`.
#[proc_macro_attribute]
pub fn client(options: TokenStream, input: TokenStream) -> TokenStream {
    entry(options, input, false)
}

/// Requires a concrete implementation and emits the existing SDK component export.
fn entry(options: TokenStream, input: TokenStream, server: bool) -> TokenStream {
    if !options.is_empty() {
        return syn::Error::new(
            proc_macro::Span::call_site().into(),
            "entry points take no options",
        )
        .to_compile_error()
        .into();
    }
    let mut item = syn::parse_macro_input!(input as syn::ItemImpl);
    let expected = if server { "Experience" } else { "ClientPart" };
    if !item.generics.params.is_empty()
        || !item
            .trait_
            .as_ref()
            .is_some_and(|(_, path, _)| path.segments.last().is_some_and(|s| s.ident == expected))
    {
        return syn::Error::new_spanned(
            &item,
            format!("expected impl {expected} for a concrete type"),
        )
        .to_compile_error()
        .into();
    }
    if server {
        for member in &item.items {
            if let syn::ImplItem::Fn(function) = member
                && ["register", "registration", "registry"]
                    .iter()
                    .any(|name| function.sig.ident == *name)
            {
                return syn::Error::new_spanned(function, "declare content in shared.rs; #[server] generates registration and the shared registry")
                    .to_compile_error().into();
            }
        }
        item.items.push(syn::parse_quote! {
            /// Registers content generated from the project's typed declarations.
            fn registration() -> ::core::result::Result<::experience_sdk::server::Registration, ::experience_sdk::server::GuestError> {
                ::core::result::Result::Ok(crate::shared::registration())
            }
        });
        item.items.push(syn::parse_quote! {
            /// Confirms the exact shared definitions before authored client callbacks start.
            fn registry() -> ::core::option::Option<&'static ::experience_sdk::message::Registry> {
                ::core::option::Option::Some(&crate::shared::REGISTRY)
            }
        });
    }
    let syn::Type::Path(ty) = item.self_ty.as_ref() else {
        return syn::Error::new_spanned(&item.self_ty, "expected a type name")
            .to_compile_error()
            .into();
    };
    if ty.qself.is_some()
        || ty.path.segments.len() != 1
        || !ty.path.segments[0].arguments.is_empty()
    {
        return syn::Error::new_spanned(ty, "use an unqualified type name")
            .to_compile_error()
            .into();
    }
    let name = &ty.path.segments[0].ident;
    let export = if server {
        quote!(::experience_sdk::export_experience!(#name);)
    } else {
        quote!(::experience_sdk::export_client_part!(#name);)
    };
    quote!(#item #export).into()
}
