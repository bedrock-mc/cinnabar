//! Java entity types and the Bedrock entity Geyser spawns for each, read from the builder
//! chains of Geyser's `VanillaEntities.java` and the shared definitions of
//! `BedrockEntityDefinitions.java`.
//!
//! Geyser's semantics (its `VanillaEntityType.Builder`, at the pinned commit): `.type(T)` sets
//! the Java type and clears any earlier `.bedrockIdentifier`; `.bedrockDefinition(D)` wins over
//! `.bedrockIdentifier("id")`, which wins over the Java type's own identifier; `inherited`
//! copies only size, offset and metadata translators from its parent, never the Bedrock
//! entity; and `.build(false)` registers nothing. A Java type `EntityType.FOO` is
//! `minecraft:foo` (MCProtocolLib names its constants after the lowercased Java ids), and an
//! identifier without a namespace is in `minecraft`.
//!
//! Anything the parser does not recognize fails the generator: a statement that mentions the
//! builder but is not one of the parsed shapes, an unexpected argument, or a declared entity
//! field that no parsed statement assigns exactly once.

use super::Result;
use std::collections::{BTreeMap, HashMap, HashSet};

/// A Java source token; comments and whitespace are dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    Str(String),
    /// A number or character literal; never inspected.
    Literal,
    Punct(char),
}

/// One statement: the tokens between two `;`, `{` or `}` outside parentheses, and the
/// character that ended it.
struct Statement<'a> {
    tokens: &'a [Token],
    end: char,
}

/// `(Java id, Bedrock id)` of every Java entity type `vanilla` registers, in file order.
pub(super) fn entity_pairs(vanilla: &str, definitions: &str) -> Result<Vec<(String, String)>> {
    let definitions = bedrock_definitions(definitions)?;
    let tokens = tokenize(vanilla)?;
    let statements = statements(&tokens);
    let helpers = helpers(&statements, &definitions)?;

    let mut fields = BTreeMap::new();
    for statement in &statements {
        if let Some(name) = field(statement, "VanillaEntityType") {
            fields.insert(name.to_owned(), 0usize);
        }
    }
    let mut pairs = Vec::new();
    let mut depth = 0usize;
    let mut helper_depth = None;
    for statement in &statements {
        let tokens = statement.tokens;
        let in_helper = helper_depth.is_some();
        match statement.end {
            '{' => {
                depth += 1;
                if helper_depth.is_none() && helper_declaration(tokens).is_some() {
                    helper_depth = Some(depth);
                }
            }
            '}' => {
                if helper_depth == Some(depth) {
                    helper_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        if in_helper || tokens.is_empty() {
            continue;
        }
        let Some((name, value)) = assignment(tokens) else {
            if mentions_builder(tokens) {
                return Err(unparsed("statement", tokens));
            }
            continue;
        };
        let parsed = if builder_head(value).is_some() {
            Some(chain(value, &definitions, None)?.resolve()?)
        } else if let Some(Token::Ident(callee)) = value.first()
            && let Some(helper) = helpers.get(callee.as_str())
        {
            Some(Some(helper.call(value)?))
        } else {
            None
        };
        match parsed {
            Some(pair) => {
                if let Some(count) = fields.get_mut(name) {
                    *count += 1;
                }
                pairs.extend(pair);
            }
            None if mentions_builder(value) || fields.contains_key(name) => {
                return Err(unparsed(&format!("assignment to {name}"), tokens));
            }
            None => {}
        }
    }
    if let Some((name, count)) = fields.iter().find(|(_, count)| **count != 1) {
        return Err(format!(
            "VanillaEntities.{name} is assigned {count} times by parsed statements"
        )
        .into());
    }
    let mut seen = HashSet::new();
    for (java, _) in &pairs {
        if !seen.insert(java.as_str()) {
            return Err(format!("{java} is registered twice").into());
        }
    }
    Ok(pairs)
}

/// `BedrockEntityDefinitions` constants: each `NAME = BedrockEntityDefinition.ofVanilla(
/// Identifier.of(..))` to its Bedrock id; every declared constant must be assigned once.
fn bedrock_definitions(source: &str) -> Result<HashMap<String, String>> {
    let tokens = tokenize(source)?;
    let statements = statements(&tokens);
    let mut declared = Vec::new();
    let mut definitions = HashMap::new();
    for statement in &statements {
        if let Some(name) = field(statement, "BedrockEntityDefinition") {
            declared.push(name.to_owned());
            continue;
        }
        let Some((name, value)) = assignment(statement.tokens) else {
            continue;
        };
        let mut cursor = Cursor::new(value);
        let identifier = cursor
            .expect_path(&["BedrockEntityDefinition", "ofVanilla"])
            .then(|| cursor.call())
            .flatten()
            .and_then(identifier_of)
            .filter(|_| cursor.done())
            .ok_or_else(|| unparsed(&format!("definition {name}"), value))?;
        if definitions.insert(name.to_owned(), identifier).is_some() {
            return Err(format!("BedrockEntityDefinitions.{name} is assigned twice").into());
        }
    }
    if let Some(name) = declared
        .iter()
        .find(|name| !definitions.contains_key(*name))
    {
        return Err(format!("BedrockEntityDefinitions.{name} has no parsed definition").into());
    }
    Ok(definitions)
}

/// `Identifier.of("path")` or `Identifier.of("namespace", "path")` as a full id.
fn identifier_of(tokens: &[Token]) -> Option<String> {
    let mut cursor = Cursor::new(tokens);
    if !cursor.expect_path(&["Identifier", "of"]) {
        return None;
    }
    let args = arguments(cursor.call()?);
    if !cursor.done() {
        return None;
    }
    match args.as_slice() {
        [[Token::Str(path)]] => Some(namespaced(path)),
        [[Token::Str(namespace)], [Token::Str(path)]] => Some(format!("{namespace}:{path}")),
        _ => None,
    }
}

fn namespaced(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// The Java type a builder's `.type(..)` names.
#[derive(Clone, Debug)]
enum JavaType {
    Id(String),
    /// The helper method's `EntityType` parameter.
    Parameter,
}

/// A parsed builder chain, before the Java type of a helper template is supplied.
#[derive(Clone, Debug)]
struct Chain {
    java: Option<JavaType>,
    bedrock_identifier: Option<String>,
    definition: Option<String>,
    register: bool,
}

impl Chain {
    /// The registered `(Java, Bedrock)` pair, `None` for `.build(false)`.
    fn resolve(&self) -> Result<Option<(String, String)>> {
        if !self.register {
            return Ok(None);
        }
        let java = match &self.java {
            Some(JavaType::Id(id)) => id.clone(),
            Some(JavaType::Parameter) => return Err("helper template used directly".into()),
            None => return Err("registered builder without .type(..)".into()),
        };
        let bedrock = self
            .definition
            .clone()
            .or_else(|| self.bedrock_identifier.clone())
            .unwrap_or_else(|| java.clone());
        Ok(Some((java, bedrock)))
    }
}

/// A `private static VanillaEntityType<..> name(.., EntityType param, ..) { return chain; }`
/// helper: the position of its `EntityType` parameter and its chain.
struct Helper {
    name: String,
    parameter: usize,
    chain: Chain,
}

impl Helper {
    /// The pair registered by `name(args)`, whose `EntityType` argument is `EntityType.FOO`.
    fn call(&self, tokens: &[Token]) -> Result<(String, String)> {
        let mut cursor = Cursor::new(tokens);
        let parsed = cursor
            .expect_path(&[self.name.as_str()])
            .then(|| cursor.call())
            .flatten()
            .filter(|_| cursor.done())
            .and_then(|args| {
                let args = arguments(args);
                java_type(args.get(self.parameter)?)
            })
            .ok_or_else(|| unparsed(&format!("call of {}", self.name), tokens))?;
        let mut chain = self.chain.clone();
        chain.java = Some(JavaType::Id(parsed));
        chain
            .resolve()?
            .ok_or_else(|| format!("{} builds an unregistered type", self.name).into())
    }
}

/// The helper methods returning a `VanillaEntityType`, by name.
fn helpers(
    statements: &[Statement],
    definitions: &HashMap<String, String>,
) -> Result<HashMap<String, Helper>> {
    let mut helpers = HashMap::new();
    for (index, statement) in statements.iter().enumerate() {
        if statement.end != '{' {
            continue;
        }
        let Some((name, parameters)) = helper_declaration(statement.tokens) else {
            continue;
        };
        let parameter = parameters
            .iter()
            .position(|parameter| {
                matches!(parameter, [Token::Ident(kind), Token::Ident(_)] if kind == "EntityType")
            })
            .ok_or_else(|| format!("helper {name} takes no EntityType parameter"))?;
        let Some(Token::Ident(parameter_name)) = parameters[parameter].get(1) else {
            unreachable!("matched above");
        };
        let body = statements
            .get(index + 1)
            .filter(|body| body.end == ';')
            .and_then(|body| match body.tokens.split_first() {
                Some((Token::Ident(keyword), chain)) if keyword == "return" => Some(chain),
                _ => None,
            })
            .filter(|_| {
                statements
                    .get(index + 2)
                    .is_some_and(|close| close.end == '}' && close.tokens.is_empty())
            })
            .ok_or_else(|| format!("helper {name} is not a single `return` of a builder chain"))?;
        let chain = chain(body, definitions, Some(parameter_name))?;
        if !matches!(chain.java, Some(JavaType::Parameter)) {
            return Err(format!("helper {name} does not set .type({parameter_name})").into());
        }
        helpers.insert(
            name.to_owned(),
            Helper {
                name: name.to_owned(),
                parameter,
                chain,
            },
        );
    }
    Ok(helpers)
}

/// `name` and the parameters (as token lists) of a method declaration statement returning
/// `VanillaEntityType<..>`.
fn helper_declaration(tokens: &[Token]) -> Option<(&str, Vec<&[Token]>)> {
    let mut cursor = Cursor::new(tokens);
    cursor.skip_modifiers();
    if !cursor.expect_path(&["VanillaEntityType"]) {
        return None;
    }
    cursor.skip_generics();
    let Some(Token::Ident(name)) = cursor.next() else {
        return None;
    };
    let parameters = arguments(cursor.call()?);
    cursor.done().then_some((name.as_str(), parameters))
}

/// The `.method(args)` chain of a `VanillaEntityType.inherited(..)` / `.builder(..)` value.
/// `parameter` names the enclosing helper's `EntityType` parameter, if any.
fn chain(
    tokens: &[Token],
    definitions: &HashMap<String, String>,
    parameter: Option<&str>,
) -> Result<Chain> {
    let fail = || unparsed("builder chain", tokens);
    let mut cursor = Cursor::new(tokens);
    builder_head(tokens).ok_or_else(fail)?;
    cursor.expect_path(&["VanillaEntityType"]);
    cursor.expect(&Token::Punct('.'));
    cursor.skip_generics();
    cursor.next();
    cursor.call().ok_or_else(fail)?;
    let mut chain = Chain {
        java: None,
        bedrock_identifier: None,
        definition: None,
        register: false,
    };
    let mut built = false;
    while !cursor.done() {
        if built || !cursor.expect(&Token::Punct('.')) {
            return Err(fail());
        }
        let Some(Token::Ident(method)) = cursor.next() else {
            return Err(fail());
        };
        let args = arguments(cursor.call().ok_or_else(fail)?);
        match method.as_str() {
            "type" => {
                chain.java = Some(match args.as_slice() {
                    [[Token::Ident(name)]] if Some(name.as_str()) == parameter => {
                        JavaType::Parameter
                    }
                    [arg] => JavaType::Id(java_type(arg).ok_or_else(fail)?),
                    _ => return Err(fail()),
                });
                chain.bedrock_identifier = None;
            }
            "bedrockIdentifier" => match args.as_slice() {
                [[Token::Str(id)]] => chain.bedrock_identifier = Some(namespaced(id)),
                _ => return Err(fail()),
            },
            "bedrockDefinition" => {
                let definition = match args.as_slice() {
                    [[Token::Ident(class), Token::Punct('.'), Token::Ident(name)]]
                        if class == "BedrockEntityDefinitions" =>
                    {
                        definitions.get(name)
                    }
                    _ => None,
                };
                chain.definition = Some(definition.ok_or_else(fail)?.clone());
            }
            "build" => {
                chain.register = match args.as_slice() {
                    [] => true,
                    [[Token::Ident(flag)]] if flag == "true" => true,
                    [[Token::Ident(flag)]] if flag == "false" => false,
                    _ => return Err(fail()),
                };
                built = true;
            }
            _ => {}
        }
    }
    if !built {
        return Err(fail());
    }
    Ok(chain)
}

/// `EntityType.FOO` as `minecraft:foo`.
fn java_type(tokens: &[Token]) -> Option<String> {
    match tokens {
        [Token::Ident(class), Token::Punct('.'), Token::Ident(name)] if class == "EntityType" => {
            Some(format!("minecraft:{}", name.to_ascii_lowercase()))
        }
        _ => None,
    }
}

/// Whether `tokens` start `VanillaEntityType.[<..>]inherited(` or `...builder(`.
fn builder_head(tokens: &[Token]) -> Option<()> {
    let mut cursor = Cursor::new(tokens);
    if !(cursor.expect_path(&["VanillaEntityType"]) && cursor.expect(&Token::Punct('.'))) {
        return None;
    }
    cursor.skip_generics();
    match cursor.next() {
        Some(Token::Ident(method)) if method == "inherited" || method == "builder" => {
            cursor.peek_is(&Token::Punct('(')).then_some(())
        }
        _ => None,
    }
}

/// Whether `tokens` start a builder chain anywhere or set a Java type or Bedrock identity.
fn mentions_builder(tokens: &[Token]) -> bool {
    (0..tokens.len()).any(|start| {
        builder_head(&tokens[start..]).is_some()
            || matches!(
                &tokens[start..],
                [Token::Punct('.'), Token::Ident(method), Token::Punct('('), ..]
                    if matches!(method.as_str(), "type" | "bedrockIdentifier" | "bedrockDefinition")
            )
    })
}

/// `name = value` with `name` the identifier before the statement's first `=`.
fn assignment(tokens: &[Token]) -> Option<(&str, &[Token])> {
    let position = tokens
        .iter()
        .position(|token| *token == Token::Punct('='))?;
    match (
        tokens.get(position.checked_sub(1)?),
        tokens.get(position + 1),
    ) {
        (Some(Token::Ident(name)), Some(next)) if *next != Token::Punct('=') => {
            Some((name.as_str(), &tokens[position + 1..]))
        }
        _ => None,
    }
}

/// The field name of a `[modifiers] <kind>[<..>] NAME;` declaration.
fn field<'a>(statement: &Statement<'a>, kind: &str) -> Option<&'a str> {
    if statement.end != ';' {
        return None;
    }
    let mut cursor = Cursor::new(statement.tokens);
    cursor.skip_modifiers();
    if !cursor.expect_path(&[kind]) {
        return None;
    }
    cursor.skip_generics();
    match cursor.next() {
        Some(Token::Ident(name)) if cursor.done() => Some(name.as_str()),
        _ => None,
    }
}

fn unparsed(what: &str, tokens: &[Token]) -> Box<dyn std::error::Error> {
    let text: Vec<String> = tokens
        .iter()
        .take(24)
        .map(|token| match token {
            Token::Ident(text) => text.clone(),
            Token::Str(text) => format!("{text:?}"),
            Token::Literal => "<literal>".to_owned(),
            Token::Punct(c) => c.to_string(),
        })
        .collect();
    format!(
        "unparseable Geyser entity source ({what}): {}",
        text.join(" ")
    )
    .into()
}

/// Splits arguments at commas outside brackets and generics (a lambda's `->` is not a
/// closing `>`); no tokens is no arguments.
fn arguments(tokens: &[Token]) -> Vec<&[Token]> {
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut args = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Punct('(' | '<' | '[' | '{') => depth += 1,
            Token::Punct('>') if index > 0 && tokens[index - 1] == Token::Punct('-') => {}
            Token::Punct(')' | '>' | ']' | '}') => depth -= 1,
            Token::Punct(',') if depth == 0 => {
                args.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    args.push(&tokens[start..]);
    args
}

/// A forward reader over a statement's tokens.
struct Cursor<'a> {
    tokens: &'a [Token],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, at: 0 }
    }

    fn done(&self) -> bool {
        self.at == self.tokens.len()
    }

    fn next(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.at)?;
        self.at += 1;
        Some(token)
    }

    fn peek_is(&self, token: &Token) -> bool {
        self.tokens.get(self.at) == Some(token)
    }

    fn expect(&mut self, token: &Token) -> bool {
        let found = self.peek_is(token);
        self.at += usize::from(found);
        found
    }

    /// Consumes `a.b.c` when the next tokens spell exactly that path.
    fn expect_path(&mut self, path: &[&str]) -> bool {
        let start = self.at;
        for (index, part) in path.iter().enumerate() {
            let dot = index == 0 || self.expect(&Token::Punct('.'));
            if !dot || !matches!(self.next(), Some(Token::Ident(name)) if name == part) {
                self.at = start;
                return false;
            }
        }
        true
    }

    fn skip_modifiers(&mut self) {
        while matches!(
            self.tokens.get(self.at),
            Some(Token::Ident(word)) if matches!(word.as_str(), "public" | "private" | "protected" | "static" | "final")
        ) {
            self.at += 1;
        }
    }

    /// Skips a balanced `<..>` when one starts here.
    fn skip_generics(&mut self) {
        if !self.peek_is(&Token::Punct('<')) {
            return;
        }
        let mut depth = 0;
        while let Some(token) = self.next() {
            match token {
                Token::Punct('<') => depth += 1,
                Token::Punct('>') => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// The tokens inside a balanced `(..)` starting here, consuming it.
    fn call(&mut self) -> Option<&'a [Token]> {
        if !self.peek_is(&Token::Punct('(')) {
            return None;
        }
        let start = self.at + 1;
        let mut depth = 0;
        while let Some(token) = self.next() {
            match token {
                Token::Punct('(') => depth += 1,
                Token::Punct(')') => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&self.tokens[start..self.at - 1]);
                    }
                }
                _ => {}
            }
        }
        None
    }
}

/// Splits tokens into statements at `;`, `{` and `}` outside parentheses.
fn statements(tokens: &[Token]) -> Vec<Statement<'_>> {
    let mut statements = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Punct('(') => depth += 1,
            Token::Punct(')') => depth -= 1,
            Token::Punct(end @ (';' | '{' | '}')) if depth == 0 => {
                statements.push(Statement {
                    tokens: &tokens[start..index],
                    end: *end,
                });
                start = index + 1;
            }
            _ => {}
        }
    }
    statements
}

/// Java tokens without comments; string escapes keep the escaped character.
fn tokenize(source: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                loop {
                    let c = chars.next().ok_or("unterminated comment")?;
                    if previous == '*' && c == '/' {
                        break;
                    }
                    previous = c;
                }
            }
            '"' | '\'' => {
                let mut text = String::new();
                loop {
                    match chars.next().ok_or("unterminated literal")? {
                        '\\' => text.push(chars.next().ok_or("unterminated literal")?),
                        end if end == c => break,
                        other => text.push(other),
                    }
                }
                tokens.push(if c == '"' {
                    Token::Str(text)
                } else {
                    Token::Literal
                });
            }
            c if c.is_ascii_digit() => {
                while chars
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
                {
                    chars.next();
                }
                tokens.push(Token::Literal);
            }
            c if c.is_alphabetic() || c == '_' || c == '$' => {
                let mut word = String::from(c);
                while let Some(&c) = chars.peek() {
                    if !(c.is_alphanumeric() || c == '_' || c == '$') {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                tokens.push(Token::Ident(word));
            }
            c => tokens.push(Token::Punct(c)),
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests;
