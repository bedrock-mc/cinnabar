//! Length expressions for `size`/`offset`/`max_size`/`min_size`, parsed the way
//! the vanilla client does: a lower-cased token stream of numbers,
//! units and signs, where a sign holds until the next one and a number without a
//! unit is dropped. The concrete pixels of each unit come from an
//! [`AxisContext`] the layout solver fills. `fill` and `default` are whole-string
//! keywords, as is a non-scalar element (`default`).

use serde_json::Value;

/// A length unit. A `px` number is [`Unit::Px`]; the rest are the percentage
/// families JSON-UI recognises.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    Px,
    /// `%` — percent of the parent's length on this axis.
    Percent,
    /// `%c` — percent of the summed lengths of this control's children.
    PercentChildren,
    /// `%cm` — the largest child on this axis; the coefficient only gates it.
    PercentChildrenMax,
    /// `%sm` — the largest sibling on this axis; the coefficient only gates it.
    PercentSiblingMax,
    /// `%x` — percent of this control's own width.
    PercentX,
    /// `%y` — percent of this control's own height.
    PercentY,
}

/// One signed unit term of a length sum (`-65.25px`, `100%`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Term {
    /// Already-signed magnitude, e.g. `-65.25` for `- 65.25px`.
    pub coeff: f64,
    pub unit: Unit,
}

/// A parsed length: either a keyword or a sum of unit terms.
#[derive(Clone, Debug, PartialEq)]
pub enum Length {
    /// Absorbs the leftover main-axis space of a `stack_panel`; fills the parent
    /// axis elsewhere.
    Fill,
    /// The control's natural size (image `base_size`, label text extent), falling
    /// back to the parent axis when the control has no measurable content.
    Default,
    Terms(Vec<Term>),
}

/// The measured surroundings an axis expression evaluates against. Missing values
/// (a `%c` with unmeasured children) contribute zero.
#[derive(Clone, Copy, Debug, Default)]
pub struct AxisContext {
    pub parent: f64,
    pub own_width: Option<f64>,
    pub own_height: Option<f64>,
    pub children: Option<f64>,
    pub children_max: Option<f64>,
    pub sibling_max: Option<f64>,
    pub natural: Option<f64>,
}

/// The outcome of evaluating a length. `Fill` is deferred to the stack solver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resolved {
    Pixels(f64),
    Fill,
}

impl Length {
    /// `N%` of the parent axis.
    pub fn percent(value: f64) -> Self {
        Length::Terms(vec![Term {
            coeff: value,
            unit: Unit::Percent,
        }])
    }

    /// A fixed pixel length.
    pub fn pixels(value: f64) -> Self {
        Length::Terms(vec![Term {
            coeff: value,
            unit: Unit::Px,
        }])
    }

    /// Resolve to pixels; unit values absent from `ctx` count as zero. `fill`
    /// yields [`Resolved::Fill`]; `default` yields the natural size or, lacking
    /// one, the parent axis.
    pub fn eval(&self, ctx: &AxisContext) -> Resolved {
        match self {
            Length::Fill => Resolved::Fill,
            Length::Default => Resolved::Pixels(ctx.natural.unwrap_or(ctx.parent)),
            Length::Terms(terms) => {
                Resolved::Pixels(terms.iter().map(|term| term.value(ctx)).sum())
            }
        }
    }

    /// Convenience: resolve, mapping `fill` to `parent` (its meaning outside a
    /// stack's main axis).
    pub fn eval_pixels(&self, ctx: &AxisContext) -> f64 {
        match self.eval(ctx) {
            Resolved::Pixels(value) => value,
            Resolved::Fill => ctx.parent,
        }
    }

    /// Whether any term reads `unit`.
    pub fn uses(&self, unit: Unit) -> bool {
        matches!(self, Length::Terms(terms) if terms.iter().any(|term| term.unit == unit))
    }
}

impl Term {
    /// This term's pixels. A zero term contributes nothing; a `%cm`/`%sm` term is
    /// the maximum itself, whatever its coefficient or sign.
    fn value(&self, ctx: &AxisContext) -> f64 {
        if self.coeff == 0.0 {
            return 0.0;
        }
        let percent = |value: Option<f64>| self.coeff * value.unwrap_or(0.0) / 100.0;
        match self.unit {
            Unit::Px => self.coeff,
            Unit::Percent => percent(Some(ctx.parent)),
            Unit::PercentChildren => percent(ctx.children),
            Unit::PercentChildrenMax => ctx.children_max.unwrap_or(0.0),
            Unit::PercentSiblingMax => ctx.sibling_max.unwrap_or(0.0),
            Unit::PercentX => percent(ctx.own_width),
            Unit::PercentY => percent(ctx.own_height),
        }
    }
}

/// Parse a `size`/`offset` element: a number is pixels, a string is an
/// expression, anything else (`null`, a bool) is `default`.
pub fn length_from_value(value: &Value) -> Length {
    match value {
        Value::Number(number) => Length::pixels(number.as_f64().unwrap_or(0.0)),
        Value::String(text) => parse_length(text),
        _ => Length::Default,
    }
}

/// Parse a length string. Exactly `fill`/`default` are keywords; anything else is
/// a lower-cased stream of numbers, `px`/`%` units and `+`/`-` signs. A sign
/// applies to every later term until the next sign, a unit-less number is
/// dropped, and a stray modifier (`c`, `m`, `s`, `x`, `y`) is ignored.
pub fn parse_length(input: &str) -> Length {
    match input {
        "fill" => return Length::Fill,
        "default" => return Length::Default,
        _ => {}
    }
    let lower = input.to_lowercase();
    let tokens = tokenize(&lower);
    let mut terms = Vec::new();
    let mut value = 0.0;
    let mut sign = 1.0;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            Token::Number(number) => value = number,
            Token::Plus => sign = 1.0,
            Token::Minus => sign = -1.0,
            Token::Px => terms.push(Term {
                coeff: sign * value,
                unit: Unit::Px,
            }),
            Token::Percent => {
                let next = |offset: usize| tokens.get(index + offset).copied();
                let (unit, consumed) = match (next(1), next(2)) {
                    (Some(Token::Modifier('c')), Some(Token::Modifier('m'))) => {
                        (Unit::PercentChildrenMax, 2)
                    }
                    (Some(Token::Modifier('c')), _) => (Unit::PercentChildren, 1),
                    (Some(Token::Modifier('s')), Some(Token::Modifier('m'))) => {
                        (Unit::PercentSiblingMax, 2)
                    }
                    (Some(Token::Modifier('x')), _) => (Unit::PercentX, 1),
                    (Some(Token::Modifier('y')), _) => (Unit::PercentY, 1),
                    // An unknown sibling modifier (`%s`) falls back to the axis.
                    _ => (Unit::Percent, 0),
                };
                terms.push(Term {
                    coeff: sign * value,
                    unit,
                });
                index += consumed;
            }
            Token::Modifier(_) => {}
        }
        index += 1;
    }
    Length::Terms(terms)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Px,
    Percent,
    Modifier(char),
}

/// Split at the client's layout delimiters (`+ - % c m s x y px`, whitespace
/// discarded); every other run is a number read by its leading float.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut number = String::new();
    let flush = |number: &mut String, tokens: &mut Vec<Token>| {
        if !number.is_empty() {
            tokens.push(Token::Number(leading_float(number)));
            number.clear();
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let token = match ch {
            '+' => Some(Token::Plus),
            '-' => Some(Token::Minus),
            '%' => Some(Token::Percent),
            'c' | 'm' | 's' | 'x' | 'y' => Some(Token::Modifier(ch)),
            'p' if chars.peek() == Some(&'x') => {
                chars.next();
                Some(Token::Px)
            }
            ch if ch.is_whitespace() => {
                flush(&mut number, &mut tokens);
                continue;
            }
            _ => None,
        };
        match token {
            Some(token) => {
                flush(&mut number, &mut tokens);
                tokens.push(token);
            }
            None => number.push(ch),
        }
    }
    flush(&mut number, &mut tokens);
    tokens
}

/// The longest leading float of `text` (a stream extraction), else zero.
fn leading_float(text: &str) -> f64 {
    (1..=text.len())
        .rev()
        .filter(|end| text.is_char_boundary(*end))
        .find_map(|end| text[..end].parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(input: &str) -> Vec<Term> {
        match parse_length(input) {
            Length::Terms(terms) => terms,
            other => panic!("expected terms, got {other:?}"),
        }
    }

    fn px(input: &str, parent: f64) -> f64 {
        parse_length(input).eval_pixels(&AxisContext {
            parent,
            ..AxisContext::default()
        })
    }

    #[test]
    fn each_unit_parses_and_evaluates() {
        let ctx = AxisContext {
            parent: 200.0,
            own_width: Some(80.0),
            own_height: Some(45.0),
            children: Some(30.0),
            children_max: Some(12.0),
            sibling_max: Some(50.0),
            natural: Some(7.0),
        };
        assert_eq!(parse_length("4px").eval_pixels(&ctx), 4.0);
        assert_eq!(parse_length("50%").eval_pixels(&ctx), 100.0);
        assert_eq!(parse_length("100%c").eval_pixels(&ctx), 30.0);
        assert_eq!(parse_length("100%cm").eval_pixels(&ctx), 12.0);
        assert_eq!(parse_length("100%sm").eval_pixels(&ctx), 50.0);
        assert_eq!(parse_length("50%x").eval_pixels(&ctx), 40.0);
        assert_eq!(parse_length("100%y").eval_pixels(&ctx), 45.0);
        assert_eq!(parse_length("default").eval_pixels(&ctx), 7.0);
    }

    #[test]
    fn fill_is_distinct_from_pixels() {
        assert_eq!(
            parse_length("fill").eval(&AxisContext::default()),
            Resolved::Fill
        );
    }

    #[test]
    fn default_falls_back_to_parent_without_natural() {
        let ctx = AxisContext {
            parent: 120.0,
            ..AxisContext::default()
        };
        assert_eq!(Length::Default.eval_pixels(&ctx), 120.0);
    }

    #[test]
    fn subtraction_and_addition_mix_units() {
        let ctx = AxisContext {
            parent: 100.0,
            children: Some(40.0),
            ..AxisContext::default()
        };
        assert_eq!(parse_length("100% - 4px").eval_pixels(&ctx), 96.0);
        assert_eq!(parse_length("100%c + 6px").eval_pixels(&ctx), 46.0);
    }

    #[test]
    fn aspect_ratio_expression_uses_own_width() {
        // 16:9 height from an already-known width, as vanilla thumbnails do.
        let ctx = AxisContext {
            own_width: Some(200.0),
            ..AxisContext::default()
        };
        let value = parse_length("56.25%x - 65.25px + 118.5px").eval_pixels(&ctx);
        assert_eq!(value, 0.5625 * 200.0 - 65.25 + 118.5);
    }

    #[test]
    fn leading_negative_and_signed_terms() {
        assert_eq!(
            terms("-3px + 2px"),
            vec![
                Term {
                    coeff: -3.0,
                    unit: Unit::Px
                },
                Term {
                    coeff: 2.0,
                    unit: Unit::Px
                },
            ]
        );
    }

    #[test]
    fn cm_and_sm_win_over_c() {
        assert_eq!(terms("1%cm")[0].unit, Unit::PercentChildrenMax);
        assert_eq!(terms("1%sm")[0].unit, Unit::PercentSiblingMax);
        assert_eq!(terms("1%c")[0].unit, Unit::PercentChildren);
    }

    // G03: units are case-insensitive and the last sign before a term wins.
    #[test]
    fn upper_case_units_and_repeated_signs_normalize() {
        assert_eq!(px("20PX", 100.0), 20.0);
        assert_eq!(px("100% + -4px", 100.0), 96.0);
        assert_eq!(px("100%-4px-2px", 100.0), 94.0);
    }

    // A sign holds for the terms after it until the next sign.
    #[test]
    fn a_sign_carries_to_later_terms() {
        assert_eq!(px("100% - 4px 2px", 100.0), 94.0);
    }

    // A number without `px` or `%` is dropped, as the client logs a dangling number.
    #[test]
    fn unit_less_numbers_contribute_nothing() {
        assert_eq!(px("10", 100.0), 0.0);
        assert_eq!(px("50% + 10", 100.0), 50.0);
        assert_eq!(px("", 100.0), 0.0);
    }

    // G06/G07: a maximum term ignores its coefficient and sign; zero drops it.
    #[test]
    fn maximum_terms_ignore_their_coefficient() {
        let ctx = AxisContext {
            children_max: Some(40.0),
            sibling_max: Some(40.0),
            ..AxisContext::default()
        };
        assert_eq!(parse_length("50%cm").eval_pixels(&ctx), 40.0);
        assert_eq!(parse_length("50%sm").eval_pixels(&ctx), 40.0);
        assert_eq!(parse_length("-100%cm").eval_pixels(&ctx), 40.0);
        assert_eq!(parse_length("0%cm").eval_pixels(&ctx), 0.0);
    }

    #[test]
    fn non_scalar_elements_are_default() {
        assert_eq!(length_from_value(&Value::Null), Length::Default);
        assert_eq!(length_from_value(&Value::Bool(true)), Length::Default);
        assert_eq!(
            length_from_value(&serde_json::json!(7)),
            Length::pixels(7.0)
        );
    }
}
