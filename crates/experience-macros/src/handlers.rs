//! Typed handlers collected by the two entry attributes.

use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl};

/// Moves authored message methods to an inherent impl and emits the ABI adapter's router.
pub(super) fn generate(item: &mut ItemImpl, server: bool) -> syn::Result<proc_macro2::TokenStream> {
    let mut methods = Vec::new();
    let mut routes = Vec::new();
    let mut remaining = Vec::new();
    for member in std::mem::take(&mut item.items) {
        let ImplItem::Fn(mut method) = member else {
            remaining.push(member);
            continue;
        };
        if method.sig.ident == "client_message" || method.sig.ident == "dispatch" {
            return Err(syn::Error::new_spanned(
                method,
                "use #[on_message] with a typed message",
            ));
        }
        if !method
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("on_message"))
        {
            remaining.push(ImplItem::Fn(method));
            continue;
        }
        for attr in &method.attrs {
            if attr.path().is_ident("on_message") {
                attr.meta.require_path_only()?;
            } else if !attr.path().is_ident("doc") {
                return Err(syn::Error::new_spanned(
                    attr,
                    "message handlers support only documentation attributes",
                ));
            }
        }
        method
            .attrs
            .retain(|attr| !attr.path().is_ident("on_message"));
        let sig = &method.sig;
        let arity = if server { 3 } else { 1 };
        if sig.inputs.len() != arity
            || sig.receiver().is_some()
            || !sig.generics.params.is_empty()
            || sig.generics.where_clause.is_some()
            || sig.asyncness.is_some()
            || sig.unsafety.is_some()
            || sig.abi.is_some()
            || sig.variadic.is_some()
        {
            return Err(syn::Error::new_spanned(
                sig,
                if server {
                    "expected fn handler(ctx: &Callback, player: PlayerId, message: M) -> Result<(), GuestError>"
                } else {
                    "expected fn handler(message: M)"
                },
            ));
        }
        let Some(FnArg::Typed(argument)) = sig.inputs.last() else {
            unreachable!()
        };
        let ty = &argument.ty;
        let name = &sig.ident;
        routes.push(if server {
            quote! {
                if <#ty as ::experience_sdk::message::Message>::CHANNEL.id == channel {
                    let message = ::experience_sdk::server::__private::decode::<#ty>(schema, payload)?;
                    let handler: fn(&::experience_sdk::server::Callback, ::experience_sdk::server::PlayerId, #ty)
                        -> ::core::result::Result<(), ::experience_sdk::server::GuestError> = Self::#name;
                    return handler(ctx, player, message);
                }
            }
        } else {
            quote! {
                if <#ty as ::experience_sdk::message::Message>::CHANNEL.id == channel {
                    let message = ::experience_sdk::client::__private::decode::<#ty>(record)?;
                    let handler: fn(#ty) = Self::#name;
                    handler(message);
                    return ::core::result::Result::Ok(());
                }
            }
        });
        // A repeated message type must fail compilation rather than shadow a handler.
        methods.push((ty.clone(), method));
    }
    item.items = remaining;
    let ty = &item.self_ty;
    let uniqueness = methods.iter().map(|(message, _)| {
        quote! {
            impl ::experience_sdk::message::__private::Handles<#message> for #ty {}
        }
    });
    let methods = methods.iter().map(|(_, method)| method);
    let dispatch = if server {
        quote! {
            impl ::experience_sdk::server::__private::Dispatch for #ty {
                /// Validates and routes a request to its typed handler.
                fn dispatch(ctx: &::experience_sdk::server::Callback, player: ::experience_sdk::server::PlayerId,
                    channel: ::std::string::String, schema: u16, payload: ::std::vec::Vec<::experience_sdk::Value>)
                    -> ::core::result::Result<(), ::experience_sdk::server::GuestError> {
                    #(#routes)*
                    let _ = (&ctx, &player, &channel, &schema, &payload);
                    ::core::result::Result::Err(::experience_sdk::server::GuestError::Rejected("unknown message channel".into()))
                }
            }
        }
    } else {
        quote! {
            impl ::experience_sdk::client::__private::Dispatch for #ty {
                /// Validates and routes a reply to its typed handler.
                fn dispatch(channel: ::std::string::String, record: ::std::vec::Vec<::experience_sdk::Value>)
                    -> ::core::result::Result<(), ::std::string::String> {
                    #(#routes)*
                    let _ = (&channel, &record);
                    ::core::result::Result::Err("unknown message channel".into())
                }
            }
        }
    };
    Ok(quote! { impl #ty { #(#methods)* } #(#uniqueness)* #dispatch })
}
