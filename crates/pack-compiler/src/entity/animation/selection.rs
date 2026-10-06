//! Render-controller selection expressions (`A ? B : C`, `Array.name[index]`) expanded into the
//! leaves they can select, each with the condition that selects it.

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

/// Expands expressions whose leaves are `<prefix><alias>` references resolved by `resolve`.
pub(super) struct Selector<'a, T> {
    /// Lowercase leaf prefix such as `texture.`.
    pub prefix: &'static str,
    /// Lowercased `array.name` to member expressions.
    pub arrays: std::collections::BTreeMap<String, Vec<String>>,
    /// Leaf for a lowercase alias; `None` skips the leaf.
    pub resolve: &'a dyn Fn(&str) -> Option<T>,
}

impl<T> Selector<'_, T> {
    /// Every leaf the expression can select, with the path selecting each; `None` when the
    /// expression uses a form the selector does not understand.
    pub fn leaves(&self, expression: &str) -> Option<Vec<(Vec<Step>, T)>> {
        let mut output = Vec::new();
        self.expand(expression, &mut Vec::new(), &mut output, 0)?;
        Some(output)
    }

    fn expand(
        &self,
        expression: &str,
        path: &mut Vec<Step>,
        output: &mut Vec<(Vec<Step>, T)>,
        depth: usize,
    ) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        let expression = strip_outer_parentheses(expression.trim());
        if let Some((condition, then_branch, else_branch)) = split_ternary(expression) {
            let else_branch = else_branch?;
            path.push(Step::Branch(condition.to_owned(), true));
            let then_result = self.expand(then_branch, path, output, depth + 1);
            path.pop();
            path.push(Step::Branch(condition.to_owned(), false));
            let else_result = self.expand(else_branch, path, output, depth + 1);
            path.pop();
            return (then_result.is_some() && else_result.is_some()).then_some(());
        }
        let lower = expression.to_ascii_lowercase();
        if let Some(alias) = lower.strip_prefix(self.prefix) {
            if let Some(leaf) = (self.resolve)(alias) {
                if output.len() >= MAX_SELECTOR_LEAVES {
                    return None;
                }
                output.push((path.clone(), leaf));
            }
            return Some(());
        }
        let (name, index) = split_index(expression)?;
        let members = self.arrays.get(&name.to_ascii_lowercase())?;
        for (element, member) in members.iter().enumerate() {
            path.push(Step::Element(index.to_owned(), members.len(), element));
            let result = self.expand(member, path, output, depth + 1);
            path.pop();
            result?;
        }
        Some(())
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
    use std::collections::BTreeMap;

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
        assert!(selector.leaves("variable.foo").is_none());
        let leaves = selector.leaves("Texture.a").unwrap();
        assert_eq!(leaves.len(), 1);
        assert!(condition_text(&leaves[0].0).is_none());
        assert!(selector.leaves("q ? Texture.a").is_none());
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
