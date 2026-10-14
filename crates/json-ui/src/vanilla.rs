//! The categorized names shared by vanilla JSON-UI consumers.
//!
//! Keep product extensions and protocol JSON fields in their own owners.
//! Every declaration here is checked against the pinned pack vocabulary.

macro_rules! names {
    ($type:ident { $($variant:ident = $name:literal,)+ }) => {
        /// A name in the pinned JSON-UI contract.
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum $type { $($variant,)+ }

        impl $type {
            /// Every declared name in this category.
            pub const ALL: &[Self] = &[$(Self::$variant,)+];

            /// Returns the exact name expected by the pack.
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $name,)+ }
            }
        }
    };
}

macro_rules! domains {
    ($($module:ident: $type:ident => $kind:literal,)+) => {
        $(mod $module;
        pub use $module::$type;)+

        #[cfg(test)]
        /// Visits every declaration with its vocabulary category.
        fn contract_names() -> impl Iterator<Item = (&'static str, &'static str)> {
            std::iter::empty()$(.chain($type::ALL.iter().map(|name| ($kind, name.as_str()))))+
        }
    };
}

domains! {
    reference: Reference => "reference",
    binding: Binding => "binding",
    button: Button => "button",
    renderer: Renderer => "renderer",
    variable: Variable => "variable",
    control: Control => "control",
    collection: Collection => "collection",
    factory: Factory => "factory",
    text: TextKey => "text",
}

impl Variable {
    /// Returns the variable name without the `$` prefix, as context setters expect.
    pub fn key(self) -> &'static str {
        &self.as_str()[1..]
    }
}

#[cfg(test)]
mod tests;
