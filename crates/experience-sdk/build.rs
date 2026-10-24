//! Derives the safe callback surface from the canonical WIT bindings.

use std::{env, fs, path::PathBuf};

use quote::quote;
use syn::{FnArg, ImplItem, Item, Pat, visit_mut::VisitMut};
use wit_bindgen_core::{Files, WorldGenerator, wit_parser::Resolve};

/// Generates forwarding methods without exposing raw message sends to paired mods.
fn main() {
    println!("cargo:rerun-if-changed=wit/server");
    if env::var_os("CARGO_FEATURE_SERVER").is_none() {
        return;
    }
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_path("wit/server").expect("parse server WIT");
    let world = resolve
        .select_world(&[package], Some("server"))
        .expect("server world");
    let mut files = Files::default();
    wit_bindgen_rust::Opts::default()
        .build()
        .generate(&mut resolve, world, &mut files)
        .expect("generate callback signatures");
    let source = files.iter().next().expect("generated Rust file").1;
    let file =
        syn::parse_file(std::str::from_utf8(source).expect("Rust source")).expect("parse bindings");
    let mut methods = Vec::new();
    collect(&file.items, &mut methods);
    assert!(!methods.is_empty(), "callback methods must be generated");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR")).join("callback.rs"),
        quote! {
            impl Callback<'_> { #(#methods)* }
        }
        .to_string(),
    )
    .expect("write safe callback methods");
}

/// Copies canonical method signatures and delegates their arguments to the borrowed resource.
fn collect(items: &[Item], methods: &mut Vec<proc_macro2::TokenStream>) {
    for item in items {
        if let Item::Mod(module) = item
            && let Some((_, items)) = &module.content
        {
            collect(items, methods);
        }
        let Item::Impl(implementation) = item else {
            continue;
        };
        let syn::Type::Path(ty) = implementation.self_ty.as_ref() else {
            continue;
        };
        if !ty.path.is_ident("Callback") || implementation.trait_.is_some() {
            continue;
        }
        for member in &implementation.items {
            let ImplItem::Fn(method) = member else {
                continue;
            };
            if method.sig.receiver().is_none()
                || !matches!(method.vis, syn::Visibility::Public(_))
                || ["handle", "take_handle", "send_client", "send_control"]
                    .iter()
                    .any(|name| method.sig.ident == *name)
            {
                continue;
            }
            let mut signature = method.sig.clone();
            Paths.visit_signature_mut(&mut signature);
            let name = &signature.ident;
            let args = signature.inputs.iter().filter_map(|arg| match arg {
                FnArg::Typed(arg) => match arg.pat.as_ref() {
                    Pat::Ident(name) => Some(&name.ident),
                    _ => panic!("expected named WIT argument"),
                },
                FnArg::Receiver(_) => None,
            });
            let docs = method
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("doc"));
            methods.push(quote! { #(#docs)* #[doc = "Forwards this world operation within the current callback."] pub #signature { self.0.#name(#(#args),*) } });
        }
    }
}

/// Resolves generated type paths in the SDK wrapper's module instead of the bindings module.
struct Paths;
impl VisitMut for Paths {
    /// Rewrites only generated runtime and shared WIT type references.
    fn visit_type_path_mut(&mut self, ty: &mut syn::TypePath) {
        let names: Vec<_> = ty
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if names.iter().any(|name| name == "_rt") {
            let last = ty.path.segments.last().expect("runtime type").clone();
            ty.path = match last.ident.to_string().as_str() {
                "Vec" => syn::parse_quote!(::std::vec::Vec),
                "String" => syn::parse_quote!(::std::string::String),
                _ => panic!("unsupported runtime type in callback signature"),
            };
            ty.path.segments.last_mut().unwrap().arguments = last.arguments;
        } else if names.iter().any(|name| name == "types") {
            let last = ty.path.segments.last().expect("WIT type").clone();
            ty.path = syn::parse_quote!(super::#last);
        }
        syn::visit_mut::visit_type_path_mut(self, ty);
    }
}
