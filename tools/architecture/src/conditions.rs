use std::collections::BTreeMap;

use cfg_expr::{Expression, Predicate, TargetPredicate, targets::ALL_BUILTINS};
use syn::{Attribute, Item, Meta, Token, ext::IdentExt, punctuated::Punctuated};

pub(super) type Configuration = BTreeMap<String, bool>;

#[derive(Clone, Default)]
pub(super) enum Condition {
    #[default]
    Always,
    Atom(String),
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
}

impl Condition {
    /// Adds cfg and cfg_attr restrictions to the containing module's condition.
    pub(super) fn with_attrs(&self, attrs: &[Attribute]) -> Self {
        let mut parts = vec![self.clone()];
        parts.extend(
            attrs
                .iter()
                .filter_map(|attr| attribute_condition(&attr.meta)),
        );
        Self::All(parts)
    }

    /// Adds the attributes of a declaration without applying unrelated attributes.
    pub(super) fn with_item(&self, item: &Item) -> Self {
        let attrs = match item {
            Item::Mod(item) => &item.attrs,
            Item::Use(item) => &item.attrs,
            Item::ExternCrate(item) => &item.attrs,
            Item::Struct(item) => &item.attrs,
            Item::Enum(item) => &item.attrs,
            Item::Union(item) => &item.attrs,
            Item::Type(item) => &item.attrs,
            Item::Trait(item) => &item.attrs,
            Item::TraitAlias(item) => &item.attrs,
            Item::Fn(item) => &item.attrs,
            Item::Const(item) => &item.attrs,
            Item::Static(item) => &item.attrs,
            _ => return self.clone(),
        };
        self.with_attrs(attrs)
    }

    /// Returns the active state, or the next predicate needed to decide it.
    pub(super) fn enabled(&self, configuration: &Configuration) -> Result<bool, String> {
        match self {
            Self::Always => Ok(true),
            Self::Atom(atom) => {
                if let Some(value) = configuration.get(atom) {
                    return Ok(*value);
                }
                if configuration
                    .iter()
                    .any(|(other, value)| *value && exclusive(atom, other))
                {
                    return Ok(false);
                }
                if let Some(value) = platform_fact(atom, configuration) {
                    return Ok(value);
                }
                Err(atom.clone())
            }
            Self::All(parts) => evaluate_parts(parts, configuration, false),
            Self::Any(parts) => evaluate_parts(parts, configuration, true),
            Self::Not(inner) => inner.enabled(configuration).map(|value| !value),
        }
    }
}

/// Evaluates every known operand before requesting a predicate that may be irrelevant.
fn evaluate_parts(
    parts: &[Condition],
    configuration: &Configuration,
    decisive: bool,
) -> Result<bool, String> {
    let mut unknown = None;
    for part in parts {
        match part.enabled(configuration) {
            Ok(value) if value == decisive => return Ok(value),
            Ok(_) => {}
            Err(atom) => {
                unknown.get_or_insert(atom);
            }
        }
    }
    unknown.map_or(Ok(!decisive), Err)
}

/// Determines whether any conditional declaration of a name is present.
pub(super) fn any_enabled(
    parts: &[Condition],
    configuration: &Configuration,
) -> Result<bool, String> {
    evaluate_parts(parts, configuration, true)
}

/// Parses only attributes that control whether a declaration exists.
fn attribute_condition(meta: &Meta) -> Option<Condition> {
    let Meta::List(list) = meta else { return None };
    let attribute = list.path.get_ident().map(|name| name.unraw().to_string());
    if attribute.as_deref() == Some("cfg") {
        return list.parse_args::<Meta>().ok().map(|meta| predicate(&meta));
    }
    if attribute.as_deref() == Some("cfg_attr") {
        let parts = list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .ok()?;
        let mut parts = parts.iter();
        let gate = predicate(parts.next()?);
        let required = Condition::All(parts.filter_map(attribute_condition).collect());
        return Some(Condition::Any(vec![
            Condition::Not(Box::new(gate)),
            required,
        ]));
    }
    None
}

/// Keeps cfg predicates symbolic so all relevant feature and platform choices can be inspected.
fn predicate(meta: &Meta) -> Condition {
    let name = meta
        .path()
        .segments
        .iter()
        .map(|part| part.ident.unraw().to_string())
        .collect::<Vec<_>>()
        .join("::");
    match meta {
        Meta::Path(_) => Condition::Atom(name),
        Meta::NameValue(value) => {
            if let syn::Expr::Lit(value) = &value.value {
                if let syn::Lit::Str(value) = &value.lit {
                    return Condition::Atom(format!("{name}={:?}", value.value()));
                }
            }
            Condition::Atom(name)
        }
        Meta::List(list) => {
            let parts = list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .unwrap_or_default();
            let mut predicates = parts.iter().map(predicate).collect::<Vec<_>>();
            match name.as_str() {
                "all" => Condition::All(predicates),
                "any" => Condition::Any(predicates),
                "not" if predicates.len() == 1 => Condition::Not(Box::new(predicates.remove(0))),
                _ => Condition::Atom(format!("{name}({})", list.tokens)),
            }
        }
    }
}

/// Infers fixed target properties from the built-in platforms compatible with prior choices.
fn platform_fact(atom: &str, configuration: &Configuration) -> Option<bool> {
    let predicate = platform_predicate(atom)?;
    let restrictions = configuration
        .iter()
        .filter_map(|(atom, value)| platform_predicate(atom).map(|predicate| (predicate, *value)))
        .collect::<Vec<_>>();
    let mut values = ALL_BUILTINS
        .iter()
        .filter(|target| {
            restrictions
                .iter()
                .all(|(predicate, value)| predicate.matches(*target) == *value)
        })
        .map(|target| predicate.matches(target));
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

/// Selects fixed target properties while leaving build options and unknown targets symbolic.
fn platform_predicate(atom: &str) -> Option<TargetPredicate> {
    let expression = Expression::parse(atom).ok()?;
    let Predicate::Target(predicate) = expression.predicates().next()? else {
        return None;
    };
    if matches!(
        predicate,
        TargetPredicate::Panic(_) | TargetPredicate::HasAtomic(_)
    ) || !ALL_BUILTINS.iter().any(|target| predicate.matches(target))
    {
        return None;
    }
    Some(predicate)
}

/// Prevents impossible combinations of single-valued target predicates.
fn exclusive(left: &str, right: &str) -> bool {
    if matches!((left, right), ("unix", "windows") | ("windows", "unix")) {
        return true;
    }
    let (Some((left_key, _)), Some((right_key, _))) = (left.split_once('='), right.split_once('='))
    else {
        return false;
    };
    left != right
        && left_key == right_key
        && matches!(
            left_key,
            "target_arch"
                | "target_os"
                | "target_env"
                | "target_abi"
                | "target_vendor"
                | "target_endian"
                | "target_pointer_width"
                | "panic"
        )
}
