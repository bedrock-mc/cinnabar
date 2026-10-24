//! Render-controller selection expressions (`A ? B : C`, `Array.name[index]`) expanded into the
//! leaves they can select, each with the condition that selects it.

use std::{collections::BTreeMap, sync::Arc};

const MAX_SELECTOR_LEAVES: usize = 256;
const MAX_DEPTH: usize = 8;

/// One step of the path to a leaf.
#[derive(Clone)]
pub(super) enum Step {
    /// A ternary condition and the branch taken.
    Branch(String, bool),
    /// Element `index` of an array of `len` entries selected by an index expression.
    Element(String, usize, usize),
}

/// The Molang condition under which `path` selects its leaf; `None` for an unconditional leaf.
pub(super) fn condition_text(path: &[Step]) -> Option<String> {
    let parts: Vec<String> = path
        .iter()
        .map(|step| match step {
            Step::Branch(text, true) => format!("({text})"),
            Step::Branch(text, false) => format!("!({text})"),
            // As for any Molang array: indices past the end wrap, negatives select the first.
            Step::Element(index, len, element) => {
                format!("(math.mod(math.max(math.floor(({index})), 0), {len}) == {element})")
            }
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" && "))
}

/// [`condition_text`] as a boolean test, so the texture and geometry leaves one path selects
/// compile to the same interned condition.
pub(super) fn selection_condition(path: &[Step]) -> Option<String> {
    let text = condition_text(path)?;
    Some(if matches!(path, [Step::Element(..)]) {
        text
    } else {
        format!("({text}) != 0")
    })
}

/// Expands expressions whose leaves are `<prefix><alias>` references resolved by `resolve`.
pub(super) struct Selector<'a, T> {
    /// Lowercase leaf prefix such as `texture.`.
    pub prefix: &'static str,
    /// Lowercased `array.name` to member expressions.
    pub arrays: BTreeMap<String, Vec<String>>,
    /// Resolves a lowercase alias; repeated references share the result and `None` skips it.
    pub resolve: &'a dyn Fn(&str) -> Option<T>,
}

/// One selector decision, retaining each condition or array index as a single evaluation.
pub(super) enum Selection<T> {
    Empty,
    Leaf(T),
    Branch(String, Arc<Self>, Arc<Self>),
    Array(String, Vec<Arc<Self>>),
}

/// Parsed nodes and their expanded resolved-leaf counts, keyed by source expression and depth.
type SelectionCache<T> = BTreeMap<(usize, String), (Arc<Selection<T>>, usize)>;

impl<T: Clone> Selection<T> {
    /// Collects resolved leaves with their selecting paths for texture and geometry carriers.
    fn collect(&self, path: &mut Vec<Step>, output: &mut Vec<(Vec<Step>, T)>) {
        match self {
            Self::Empty => {}
            Self::Leaf(leaf) => output.push((path.clone(), leaf.clone())),
            Self::Branch(condition, yes, no) => {
                path.push(Step::Branch(condition.clone(), true));
                yes.collect(path, output);
                path.pop();
                path.push(Step::Branch(condition.clone(), false));
                no.collect(path, output);
                path.pop();
            }
            Self::Array(index, members) => {
                let count = members.len();
                for (element, member) in members.iter().enumerate() {
                    path.push(Step::Element(index.clone(), count, element));
                    member.collect(path, output);
                    path.pop();
                }
            }
        }
    }
}

impl<T> Selector<'_, T> {
    /// Expands resolvable leaves into the conditions selecting them.
    pub fn leaves(&self, expression: &str) -> Option<Vec<(Vec<Step>, T)>>
    where
        T: Clone,
    {
        let mut output = Vec::new();
        self.tree(expression)?.collect(&mut Vec::new(), &mut output);
        Some(output)
    }

    /// Retains the decision tree so material selectors evaluate each decision exactly once.
    pub fn tree(&self, expression: &str) -> Option<Arc<Selection<T>>> {
        self.expand(expression, &mut 0, 0, &mut BTreeMap::new())
    }

    /// Shares repeated expressions at the same depth while preserving the expanded leaf limit.
    fn expand(
        &self,
        expression: &str,
        leaves: &mut usize,
        depth: usize,
        cache: &mut SelectionCache<T>,
    ) -> Option<Arc<Selection<T>>> {
        if depth > MAX_DEPTH {
            return None;
        }
        let expression = strip_outer_parentheses(expression.trim());
        let key = (depth, expression.to_owned());
        if let Some((node, count)) = cache.get(&key) {
            if leaves.checked_add(*count)? > MAX_SELECTOR_LEAVES {
                return None;
            }
            *leaves += *count;
            return Some(Arc::clone(node));
        }
        let start = *leaves;
        let node = Arc::new(self.parse(expression, leaves, depth, cache)?);
        cache.insert(key, (Arc::clone(&node), *leaves - start));
        Some(node)
    }

    /// Parses a cache miss and collapses unresolved subtrees before retaining their parents.
    fn parse(
        &self,
        expression: &str,
        leaves: &mut usize,
        depth: usize,
        cache: &mut SelectionCache<T>,
    ) -> Option<Selection<T>> {
        if let Some((condition, yes, no)) = split_ternary(expression) {
            let yes = self.expand(yes, leaves, depth + 1, cache)?;
            let no = self.expand(no?, leaves, depth + 1, cache)?;
            return Some(
                if matches!(yes.as_ref(), Selection::Empty)
                    && matches!(no.as_ref(), Selection::Empty)
                {
                    Selection::Empty
                } else {
                    Selection::Branch(condition.to_owned(), yes, no)
                },
            );
        }
        let lower = expression.to_ascii_lowercase();
        if let Some(alias) = lower.strip_prefix(self.prefix) {
            return match (self.resolve)(alias) {
                None => Some(Selection::Empty),
                Some(leaf) => {
                    if *leaves >= MAX_SELECTOR_LEAVES {
                        return None;
                    }
                    *leaves += 1;
                    Some(Selection::Leaf(leaf))
                }
            };
        }
        if self.prefix == "texture."
            && lower.strip_prefix("variable.").is_some_and(|name| {
                !name.is_empty()
                    && name.split('.').all(|part| {
                        let mut chars = part.chars();
                        chars
                            .next()
                            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
                            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    })
            })
        {
            return Some(Selection::Empty);
        }
        let (name, index) = split_index(expression)?;
        let members = self.arrays.get(&name.to_ascii_lowercase())?;
        let members = members
            .iter()
            .map(|member| self.expand(member, leaves, depth + 1, cache))
            .collect::<Option<Vec<_>>>()?;
        Some(
            if members
                .iter()
                .all(|member| matches!(member.as_ref(), Selection::Empty))
            {
                Selection::Empty
            } else {
                Selection::Array(index.to_owned(), members)
            },
        )
    }
}

pub(super) fn strip_outer_parentheses(mut text: &str) -> &str {
    while text.starts_with('(') && text.ends_with(')') {
        let mut depth = 0i32;
        let mut encloses = true;
        for (offset, ch) in text.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth == 0 && offset + ch.len_utf8() < text.len() {
                encloses = false;
                break;
            }
        }
        if !encloses {
            break;
        }
        text = text[1..text.len() - 1].trim();
    }
    text
}

/// `(condition, then, else)` of a top-level `?:`; the else part is `None` for a bare `?`.
pub(super) fn split_ternary(text: &str) -> Option<(&str, &str, Option<&str>)> {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut question = None;
    let mut nested = 0i32;
    for (offset, ch) in text.char_indices() {
        if ch == '\'' {
            quoted = !quoted;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '?' if depth == 0 => match question {
                None => question = Some(offset),
                Some(_) => nested += 1,
            },
            ':' if depth == 0 && question.is_some() => {
                if nested == 0 {
                    let question = question?;
                    return Some((
                        text[..question].trim(),
                        text[question + 1..offset].trim(),
                        Some(text[offset + 1..].trim()),
                    ));
                }
                nested -= 1;
            }
            _ => {}
        }
    }
    question.map(|question| (text[..question].trim(), text[question + 1..].trim(), None))
}

/// `name` and `index` of `name[index]`.
pub(super) fn split_index(text: &str) -> Option<(&str, &str)> {
    let open = text.find('[')?;
    let inner = text.strip_suffix(']')?;
    Some((text[..open].trim(), inner[open + 1..].trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(resolve: &dyn Fn(&str) -> Option<u32>) -> Selector<'_, u32> {
        Selector {
            prefix: "texture.",
            arrays: BTreeMap::from([(
                "array.skins".into(),
                vec!["Texture.a".into(), "Texture.b".into()],
            )]),
            resolve,
        }
    }

    fn resolve(alias: &str) -> Option<u32> {
        match alias {
            "a" => Some(0),
            "b" => Some(1),
            _ => None,
        }
    }

    #[test]
    fn ternaries_and_arrays_expand_into_conditioned_leaves() {
        let selector = selector(&resolve);
        let leaves = selector
            .leaves("query.is_baby ? Texture.a : Array.skins[query.variant]")
            .unwrap();
        let texts: Vec<_> = leaves
            .iter()
            .map(|(path, leaf)| (condition_text(path).unwrap(), *leaf))
            .collect();
        assert_eq!(texts.len(), 3);
        assert_eq!(texts[0], ("(query.is_baby)".into(), 0));
        assert!(texts[1].0.starts_with("!(query.is_baby) && (math.mod("));
        assert_eq!(texts[2].1, 1);
    }

    #[test]
    fn unsupported_atoms_reject_and_plain_aliases_are_unconditional() {
        let selector = selector(&resolve);
        assert!(selector.leaves("query.foo").is_none());
        let leaves = selector.leaves("Texture.a").unwrap();
        assert_eq!(leaves.len(), 1);
        assert!(condition_text(&leaves[0].0).is_none());
        assert!(selector.leaves("q ? Texture.a").is_none());
    }

    #[test]
    fn unresolved_texture_variable_keeps_the_static_trim_branch_condition() {
        let selector = selector(&resolve);
        let leaves = selector
            .leaves("variable.has_trim ? variable.trim_path : Texture.a")
            .expect("unresolved runtime texture variables must not discard static siblings");
        assert_eq!(leaves.len(), 1);
        assert_eq!(
            condition_text(&leaves[0].0).as_deref(),
            Some("!(variable.has_trim)")
        );
        assert_eq!(leaves[0].1, 0);
        assert!(selector.leaves("variable.trim_path + 1").is_none());
        assert!(selector.leaves("variable.").is_none());
        assert!(selector.leaves("variable.trim_path[0]").is_none());
    }

    #[test]
    fn selector_leaf_capacity_is_an_exact_bound() {
        let mut selector = selector(&resolve);
        selector.arrays.insert(
            "array.skins".into(),
            vec!["Texture.a".into(); MAX_SELECTOR_LEAVES],
        );
        assert_eq!(
            selector.leaves("Array.skins[query.variant]").unwrap().len(),
            MAX_SELECTOR_LEAVES
        );
        selector
            .arrays
            .get_mut("array.skins")
            .unwrap()
            .push("Texture.a".into());
        assert!(selector.leaves("Array.skins[query.variant]").is_none());
    }

    #[test]
    fn ternary_split_respects_nesting_brackets_and_quotes() {
        assert_eq!(
            split_ternary("a ? b ? c : d : e"),
            Some(("a", "b ? c : d", Some("e")))
        );
        assert_eq!(
            split_ternary("query.x('m?n') ? Array.t[q ? 1 : 0] : d"),
            Some(("query.x('m?n')", "Array.t[q ? 1 : 0]", Some("d")))
        );
        assert_eq!(strip_outer_parentheses("(a ? b : c)"), "a ? b : c");
        assert_eq!(strip_outer_parentheses("(a) ? (b) : c"), "(a) ? (b) : c");
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    use std::{cell::Cell, collections::BTreeMap};

    #[test]
    fn repeated_unresolved_arrays_stay_compact() {
        let calls = Cell::new(0);
        let resolve = |_: &str| {
            calls.set(calls.get() + 1);
            None::<u32>
        };
        let mut arrays = BTreeMap::new();
        for level in 0..5 {
            let member = if level == 0 {
                "Material.missing".to_owned()
            } else {
                format!("Array.a{}[0]", level - 1)
            };
            arrays.insert(format!("array.a{level}"), vec![member; 4]);
        }
        let selector = Selector {
            prefix: "material.",
            arrays,
            resolve: &resolve,
        };
        let tree = selector.tree("Array.a4[0]").unwrap();
        assert!(matches!(tree.as_ref(), Selection::Empty));
        assert_eq!(
            calls.get(),
            1,
            "repeated missing aliases must not expand into repeated retained work"
        );
        assert!(selector.leaves("Array.a4[0]").unwrap().is_empty());
    }

    #[test]
    fn shared_subtrees_preserve_the_expanded_leaf_admission() {
        let resolve = |_: &str| Some(0);
        let selector = Selector {
            prefix: "material.",
            resolve: &resolve,
            arrays: BTreeMap::from([
                ("array.inner".into(), vec!["Material.a".into(); 2]),
                (
                    "array.outer".into(),
                    vec!["Array.inner[0]".into(); MAX_SELECTOR_LEAVES / 2 + 1],
                ),
            ]),
        };
        assert!(selector.tree("Array.outer[0]").is_none());
    }
}
