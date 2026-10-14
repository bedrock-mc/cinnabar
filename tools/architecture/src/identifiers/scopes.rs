// Attribute-owning nodes in the pinned syn API; its exact dependency pin guards this inventory.
#[cfg(test)]
pub(super) const SYN_SCHEMA_VERSION: &str = "2.0.118";

/// Defines attribute-scoped visitor methods, with optional handlers for identifier-specific work.
macro_rules! attribute_scopes {
    (@node $lifetime:lifetime, $method:ident, $node:ident) => {
        /// Visits this node within its own test-only attribute scope.
        fn $method(&mut self, node: &$lifetime syn::$node) {
            self.with_attributes(&node.attrs, |scan| syn::visit::$method(scan, node));
        }
    };
    (@node $lifetime:lifetime, $method:ident, $node:ident, $handler:ident) => {
        /// Visits this node within its own test-only attribute scope.
        fn $method(&mut self, node: &$lifetime syn::$node) {
            self.with_attributes(&node.attrs, |scan| scan.$handler(node));
        }
    };
    ($lifetime:lifetime) => {
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_arm, Arm);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_bare_fn_arg, BareFnArg);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_bare_variadic, BareVariadic);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_const_param, ConstParam);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_derive_input, DeriveInput);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_array, ExprArray);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_assign, ExprAssign);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_async, ExprAsync);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_await, ExprAwait);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_binary, ExprBinary);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_block, ExprBlock);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_break, ExprBreak);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_call, ExprCall, scan_call);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_cast, ExprCast);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_closure, ExprClosure);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_const, ExprConst);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_continue, ExprContinue);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_field, ExprField);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_for_loop, ExprForLoop);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_group, ExprGroup);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_if, ExprIf);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_index, ExprIndex);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_infer, ExprInfer);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_let, ExprLet);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_lit, ExprLit);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_loop, ExprLoop);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_macro, ExprMacro);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_match, ExprMatch);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_method_call, ExprMethodCall, scan_method_call);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_paren, ExprParen);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_path, ExprPath);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_range, ExprRange);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_raw_addr, ExprRawAddr);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_reference, ExprReference);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_repeat, ExprRepeat);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_return, ExprReturn);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_struct, ExprStruct);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_try, ExprTry);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_try_block, ExprTryBlock);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_tuple, ExprTuple);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_unary, ExprUnary);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_unsafe, ExprUnsafe);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_while, ExprWhile);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_expr_yield, ExprYield);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_field, Field);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_field_pat, FieldPat);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_field_value, FieldValue);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_file, File, scan_file);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_foreign_item_fn, ForeignItemFn);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_foreign_item_macro, ForeignItemMacro);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_foreign_item_static, ForeignItemStatic);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_foreign_item_type, ForeignItemType);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_impl_item_const, ImplItemConst, scan_impl_const);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_impl_item_fn, ImplItemFn);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_impl_item_macro, ImplItemMacro);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_impl_item_type, ImplItemType);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_const, ItemConst, scan_const);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_enum, ItemEnum);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_extern_crate, ItemExternCrate);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_fn, ItemFn);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_foreign_mod, ItemForeignMod);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_impl, ItemImpl);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_macro, ItemMacro);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_mod, ItemMod, scan_module);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_static, ItemStatic, scan_static);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_struct, ItemStruct);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_trait, ItemTrait);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_trait_alias, ItemTraitAlias);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_type, ItemType);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_union, ItemUnion);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_item_use, ItemUse);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_lifetime_param, LifetimeParam);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_local, Local);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_ident, PatIdent);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_or, PatOr);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_paren, PatParen);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_reference, PatReference);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_rest, PatRest);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_slice, PatSlice);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_struct, PatStruct);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_tuple, PatTuple);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_tuple_struct, PatTupleStruct);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_type, PatType);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_pat_wild, PatWild);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_receiver, Receiver);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_stmt_macro, StmtMacro);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_trait_item_const, TraitItemConst, scan_trait_const);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_trait_item_fn, TraitItemFn);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_trait_item_macro, TraitItemMacro);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_trait_item_type, TraitItemType);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_type_param, TypeParam);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_variadic, Variadic);
        $crate::identifiers::scopes::attribute_scopes!(@node $lifetime, visit_variant, Variant);
    };
}

pub(super) use attribute_scopes;
