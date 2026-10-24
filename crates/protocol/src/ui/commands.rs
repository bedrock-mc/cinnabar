//! AvailableCommands command tree and the chat completion catalog built on it.

use std::{collections::BTreeMap, fmt::Write as _, sync::Arc};

use bedrock_protocol_1_26_51::enums::CommandPermissionLevel;
use bedrock_protocol_1_26_51::packets::AvailableCommands;
use bedrock_protocol_1_26_51::wire;
use thiserror::Error;

use protocol_ids::{CommandArgFlag, CommandArgType};

use super::{
    ChatAutocompleteAction, ChatAutocompleteEvent, MAX_CHAT_AUTOCOMPLETE,
    MAX_CHAT_AUTOCOMPLETE_BYTES, MAX_OUTBOUND_CHAT_BYTES, UiEvent,
};

const MAX_COMMANDS: usize = 4_096;
const MAX_SOFT_ENUM_VALUES: usize = 16_384;
const MAX_USAGE_ENUM_VALUES: usize = 6;
const SELECTORS: [&str; 6] = ["@a", "@e", "@n", "@p", "@r", "@s"];
/// The chat mention that addresses every player.
pub const ALL_PLAYERS_MENTION: &str = "@here";

// Parameter type word layout: flag bits over a table index or basic type id.
const ARG_ENUM: u32 = CommandArgFlag::Enum.wire_value();
const ARG_SOFT_ENUM: u32 = CommandArgFlag::SoftEnum.wire_value();
const ARG_CHAINED: u32 = CommandArgType::ChainedCommand.wire_value();
const ARG_ENUM_INDEX_MASK: u32 = CommandArgFlag::Valid.wire_value() - 1;
const ARG_BASIC_MASK: u32 = 0xFFFF;
const PARAM_OPTION_COLLAPSE_ENUM: u8 = 1;

/// Argument shape of one command parameter, reduced to what completion needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandParamKind {
    Enum {
        name: Arc<str>,
        values: Arc<[Arc<str>]>,
    },
    SoftEnum(Arc<str>),
    Int,
    Float,
    Target,
    /// Block or world position: three whitespace-separated coordinates.
    Position,
    /// Free text that swallows the rest of the line.
    Rest,
    Word,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandParam {
    pub name: Arc<str>,
    pub optional: bool,
    pub collapse_enum: bool,
    pub kind: CommandParamKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: Arc<str>,
    pub aliases: Arc<[Arc<str>]>,
    /// Required command permission level (0 any .. 5 internal).
    pub permission: u8,
    pub overloads: Arc<[Arc<[CommandParam]>]>,
}

/// Full command list from one AvailableCommands packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandTreeEvent {
    pub commands: Arc<[CommandSpec]>,
    pub soft_enums: Arc<[NamedValues]>,
}

type NamedValues = (Arc<str>, Arc<[Arc<str>]>);

#[cfg(test)]
mod id_tests;

/// Resolves the permission name to the generated wire permission level.
fn permission_level(name: &str) -> u8 {
    match name {
        "gamedirectors" => CommandPermissionLevel::Gamedirectors,
        "admin" => CommandPermissionLevel::Admin,
        "host" => CommandPermissionLevel::Host,
        "owner" => CommandPermissionLevel::Owner,
        "internal" => CommandPermissionLevel::Internal,
        _ => CommandPermissionLevel::Any,
    }
    .to_raw()
}

/// Reduces known wire parameter types to the shapes used by completion.
fn param_kind(word: u32, enums: &[NamedValues], soft: &[NamedValues]) -> CommandParamKind {
    let index = (word & ARG_ENUM_INDEX_MASK) as usize;
    if word & ARG_ENUM != 0 {
        return enums
            .get(index)
            .map_or(CommandParamKind::Word, |(name, values)| {
                CommandParamKind::Enum {
                    name: Arc::clone(name),
                    values: Arc::clone(values),
                }
            });
    }
    if word & ARG_SOFT_ENUM != 0 {
        return soft.get(index).map_or(CommandParamKind::Word, |(name, _)| {
            CommandParamKind::SoftEnum(Arc::clone(name))
        });
    }
    if word & ARG_CHAINED != 0 {
        return CommandParamKind::Rest;
    }
    match CommandArgType::from_wire(word & ARG_BASIC_MASK) {
        Some(CommandArgType::Int | CommandArgType::WildcardInt) => CommandParamKind::Int,
        Some(CommandArgType::Float) => CommandParamKind::Float,
        Some(
            CommandArgType::Target
            | CommandArgType::StandaloneTarget
            | CommandArgType::WildcardTarget
            | CommandArgType::NonIDTarget,
        ) => CommandParamKind::Target,
        Some(CommandArgType::BlockPosition | CommandArgType::Position) => {
            CommandParamKind::Position
        }
        Some(
            CommandArgType::Message
            | CommandArgType::RawText
            | CommandArgType::TextContinuation
            | CommandArgType::JSON,
        ) => CommandParamKind::Rest,
        _ => CommandParamKind::Word,
    }
}

/// A wire string as retained text; invalid UTF-8 decodes lossily.
fn text(value: &wire::Str) -> Arc<str> {
    Arc::from(value.to_string_lossy())
}

/// Lenient: out-of-range indices degrade a parameter to a plain word; chained
/// overloads are dropped.
pub(crate) fn normalize_available_commands(packet: AvailableCommands) -> UiEvent {
    let values = &packet.enum_values;
    // Only referenced pool entries allocate text; repeated indices share that allocation.
    let mut shared_values = BTreeMap::new();
    let enums: Vec<NamedValues> = packet
        .enum_data
        .iter()
        .map(|entry| {
            let list = entry
                .values
                .iter()
                .filter_map(|index| {
                    let value = values.get(index.0 as usize)?;
                    Some(Arc::clone(
                        shared_values.entry(index.0).or_insert_with(|| text(value)),
                    ))
                })
                .collect::<Vec<Arc<str>>>();
            (text(&entry.name), Arc::from(list))
        })
        .collect();
    let soft: Vec<NamedValues> = packet
        .soft_enums
        .iter()
        .map(|entry| {
            let list = entry
                .enum_options
                .iter()
                .take(MAX_SOFT_ENUM_VALUES)
                .map(text)
                .collect::<Vec<Arc<str>>>();
            (text(&entry.enum_name), Arc::from(list))
        })
        .collect();
    let commands = packet
        .commands
        .iter()
        .take(MAX_COMMANDS)
        .filter(|command| !command.name.is_empty())
        .map(|command| {
            let aliases = usize::try_from(command.alias_enum.0)
                .ok()
                .and_then(|index| enums.get(index))
                .map(|(_, values)| Arc::clone(values))
                .unwrap_or_else(|| Arc::from(Vec::<Arc<str>>::new()));
            let overloads = command
                .overloads
                .iter()
                .filter(|overload| !overload.is_chaining)
                .map(|overload| {
                    let params = overload
                        .parameter_data
                        .iter()
                        .map(|param| CommandParam {
                            name: text(&param.name),
                            optional: param.is_optional,
                            collapse_enum: param.options.0 & PARAM_OPTION_COLLAPSE_ENUM != 0,
                            kind: param_kind(param.parse_symbol.0, &enums, &soft),
                        })
                        .collect::<Vec<_>>();
                    Arc::from(params)
                })
                .collect::<Vec<Arc<[CommandParam]>>>();
            CommandSpec {
                name: text(&command.name),
                aliases,
                permission: permission_level(&command.permission_level.to_string_lossy()),
                overloads: Arc::from(overloads),
            }
        })
        .collect::<Vec<_>>();
    UiEvent::AvailableCommands(CommandTreeEvent {
        commands: Arc::from(commands),
        soft_enums: Arc::from(soft),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAutocompleteCompletion {
    pub catalog_revision: u64,
    pub suggestions: Arc<[Arc<str>]>,
    /// Usage line for the command being typed, when the command is known.
    pub usage: Option<Arc<str>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ChatAutocompleteCatalogError {
    #[error("autocomplete input or cursor is invalid")]
    InvalidInput,
    #[error("autocomplete catalog has {count} suggestions, exceeding {max}")]
    TooManySuggestions { count: usize, max: usize },
    #[error("autocomplete catalog retains {bytes} bytes, exceeding {max}")]
    SuggestionsTooLarge { bytes: usize, max: usize },
}

/// Session facts that filter or extend completion.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompletionContext<'a> {
    pub players: &'a [Arc<str>],
    /// Local command permission level; `None` shows every command.
    pub command_permission: Option<u8>,
}

enum Take {
    Took(usize),
    /// The parameter absorbs every remaining token.
    Open,
    Mismatch,
}

#[derive(Debug, Clone, Default)]
pub struct ChatAutocompleteCatalog {
    revision: u64,
    enums: BTreeMap<Arc<str>, Vec<Arc<str>>>,
    commands: Vec<CommandSpec>,
    soft_enums: BTreeMap<Arc<str>, Vec<Arc<str>>>,
}

fn edit_values(
    values: &mut Vec<Arc<str>>,
    action: ChatAutocompleteAction,
    suggestions: &[Arc<str>],
) {
    match action {
        ChatAutocompleteAction::Add => {}
        ChatAutocompleteAction::Remove => {
            values.retain(|value| !suggestions.contains(value));
            return;
        }
        ChatAutocompleteAction::Replace => values.clear(),
    }
    for suggestion in suggestions {
        if !values.contains(suggestion) {
            values.push(Arc::clone(suggestion));
        }
    }
}

fn has_prefix(candidate: &str, prefix: &str) -> bool {
    candidate
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// Matches a completion substring using ASCII case rules.
fn contains_partial(candidate: &str, partial: &str) -> bool {
    partial.is_empty()
        || candidate
            .as_bytes()
            .windows(partial.len())
            .any(|part| part.eq_ignore_ascii_case(partial.as_bytes()))
}

impl ChatAutocompleteCatalog {
    pub fn apply(
        &mut self,
        event: ChatAutocompleteEvent,
    ) -> Result<u64, ChatAutocompleteCatalogError> {
        if let Some(values) = self.soft_enums.get_mut(&event.enum_name) {
            let mut next = values.clone();
            edit_values(&mut next, event.action, &event.suggestions);
            next.truncate(MAX_SOFT_ENUM_VALUES);
            *values = next;
            self.revision = self.revision.saturating_add(1);
            return Ok(self.revision);
        }
        let mut values = self
            .enums
            .get(&event.enum_name)
            .cloned()
            .unwrap_or_default();
        edit_values(&mut values, event.action, &event.suggestions);
        let mut next = self.enums.clone();
        if values.is_empty() {
            next.remove(&event.enum_name);
        } else {
            next.insert(event.enum_name, values);
        }
        let count = next.values().map(Vec::len).sum::<usize>();
        if count > MAX_CHAT_AUTOCOMPLETE {
            return Err(ChatAutocompleteCatalogError::TooManySuggestions {
                count,
                max: MAX_CHAT_AUTOCOMPLETE,
            });
        }
        let bytes = next
            .iter()
            .map(|(name, values)| {
                name.len() + values.iter().map(|value| value.len()).sum::<usize>()
            })
            .sum::<usize>();
        if bytes > MAX_CHAT_AUTOCOMPLETE_BYTES {
            return Err(ChatAutocompleteCatalogError::SuggestionsTooLarge {
                bytes,
                max: MAX_CHAT_AUTOCOMPLETE_BYTES,
            });
        }
        self.enums = next;
        self.revision = self.revision.saturating_add(1);
        Ok(self.revision)
    }

    /// Replaces the command tree and its soft enums.
    pub fn apply_commands(&mut self, event: CommandTreeEvent) -> u64 {
        let mut commands = event.commands.to_vec();
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        self.commands = commands;
        self.soft_enums = event
            .soft_enums
            .iter()
            .map(|(name, values)| (Arc::clone(name), values.to_vec()))
            .collect();
        self.revision = self.revision.saturating_add(1);
        self.revision
    }

    pub fn complete(
        &self,
        input: &str,
        cursor_byte: usize,
    ) -> Result<ChatAutocompleteCompletion, ChatAutocompleteCatalogError> {
        self.complete_in(input, cursor_byte, CompletionContext::default())
    }

    /// Completes command arguments or the last `@` mention before the cursor.
    /// Flat server enum values remain available without a command tree.
    pub fn complete_in(
        &self,
        input: &str,
        cursor_byte: usize,
        context: CompletionContext<'_>,
    ) -> Result<ChatAutocompleteCompletion, ChatAutocompleteCatalogError> {
        if input.len() > MAX_OUTBOUND_CHAT_BYTES
            || cursor_byte > input.len()
            || !input.is_char_boundary(cursor_byte)
        {
            return Err(ChatAutocompleteCatalogError::InvalidInput);
        }
        let before = &input[..cursor_byte];
        let token = &before[Self::replacement_start(before)..];
        let mention = is_mention(before);
        let tree = !self.commands.is_empty();
        let (mut suggestions, usage, players) = if mention {
            let mut out = Vec::new();
            if contains_partial(ALL_PLAYERS_MENTION, token) {
                out.push(Arc::from(ALL_PLAYERS_MENTION));
            }
            extend_players(&mut out, context.players, &token[1..], true);
            (out, None, true)
        } else if tree {
            self.complete_tree(before, context)
        } else {
            let mut flat: Vec<Arc<str>> = Vec::new();
            for suggestion in self.enums.values().flatten() {
                if contains_partial(suggestion, token) && !flat.contains(suggestion) {
                    flat.push(Arc::clone(suggestion));
                }
            }
            (flat, None, false)
        };
        if players {
            let partial = if mention { &token[1..] } else { token };
            suggestions.sort_by(|a, b| {
                let a = Self::suggestion_text(before, a);
                let b = Self::suggestion_text(before, b);
                has_prefix(b, partial)
                    .cmp(&has_prefix(a, partial))
                    .then_with(|| a.cmp(b))
            });
            suggestions.dedup();
        }
        finish(&mut suggestions, token, tree && !players);
        Ok(ChatAutocompleteCompletion {
            catalog_revision: self.revision,
            suggestions: Arc::from(suggestions),
            usage,
        })
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Finds the start of a mention or a command token, keeping quoted names whole.
    pub fn replacement_start(before: &str) -> usize {
        if is_mention(before) {
            return before.rfind('@').expect("a mention contains @");
        }
        let token = command_tokens(before).pop().unwrap_or("");
        before.len() - token.len()
    }

    /// Shows player names without the mention marker, while retaining `@here`.
    pub fn suggestion_text<'a>(before: &str, suggestion: &'a str) -> &'a str {
        if is_mention(before) && suggestion != ALL_PLAYERS_MENTION {
            suggestion.strip_prefix('@').unwrap_or(suggestion)
        } else {
            suggestion
        }
    }

    /// Returns the localization key for a built-in selector or all-player mention.
    pub fn suggestion_description(before: &str, suggestion: &str) -> Option<String> {
        if is_mention(before) {
            (suggestion == ALL_PLAYERS_MENTION)
                .then(|| "chat.mentions.autocomplete.allPlayers".to_owned())
        } else {
            SELECTORS
                .contains(&suggestion)
                .then(|| format!("commands.autocomplete.{}", &suggestion[1..]))
        }
    }

    /// Finds the typed substring in the displayed suggestion using ASCII case rules.
    pub fn suggestion_match(before: &str, suggestion: &str) -> Option<std::ops::Range<usize>> {
        let token_start = Self::replacement_start(before);
        let mut partial = &before[token_start..];
        if is_mention(before) && suggestion != ALL_PLAYERS_MENTION {
            partial = &partial[1..];
        } else if token_start == 0 {
            partial = partial.strip_prefix('/').unwrap_or(partial);
        }
        if partial.is_empty() {
            return None;
        }
        let text = Self::suggestion_text(before, suggestion);
        let start = text
            .as_bytes()
            .windows(partial.len())
            .position(|part| part.eq_ignore_ascii_case(partial.as_bytes()))?;
        Some(start..start + partial.len())
    }

    fn find(&self, name: &str, context: CompletionContext<'_>) -> Option<&CommandSpec> {
        self.commands.iter().find(|command| {
            visible(command, context)
                && (command.name.eq_ignore_ascii_case(name)
                    || command
                        .aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(name)))
        })
    }

    fn complete_tree(
        &self,
        before: &str,
        context: CompletionContext<'_>,
    ) -> (Vec<Arc<str>>, Option<Arc<str>>, bool) {
        let Some(line) = before.strip_prefix('/') else {
            return (Vec::new(), None, false);
        };
        let mut tokens = command_tokens(line);
        let partial = tokens.pop().unwrap_or("");
        tokens.retain(|token| !token.is_empty());
        let mut out = Vec::new();
        let Some((&name, args)) = tokens.split_first() else {
            for command in self.commands.iter().filter(|c| visible(c, context)) {
                for candidate in std::iter::once(&command.name).chain(command.aliases.iter()) {
                    if contains_partial(candidate, partial) {
                        out.push(Arc::from(format!("/{candidate}")));
                    }
                }
            }
            let usage = self
                .find(partial, context)
                .map(|command| usage_of(command, command.overloads.first()));
            return (out, usage, false);
        };
        let Some(command) = self.find(name, context) else {
            return (out, None, false);
        };
        let mut shown = None;
        let mut players = false;
        for overload in command.overloads.iter() {
            let Some(current) = self.match_overload(overload, args) else {
                continue;
            };
            shown.get_or_insert(overload);
            let mut index = current;
            while let Some(param) = overload.get(index) {
                players |= matches!(param.kind, CommandParamKind::Target);
                self.param_values(param, partial, context, &mut out);
                if !param.optional {
                    break;
                }
                index += 1;
            }
        }
        let usage = usage_of(command, shown.or_else(|| command.overloads.first()));
        (out, Some(usage), players)
    }

    /// Index of the parameter the next token belongs to, or `None` when
    /// the finished tokens contradict this overload.
    fn match_overload(&self, params: &[CommandParam], done: &[&str]) -> Option<usize> {
        let (mut param_index, mut token_index) = (0, 0);
        while token_index < done.len() {
            let param = params.get(param_index)?;
            match self.take(param, &done[token_index..]) {
                Take::Took(count) => {
                    token_index += count;
                    param_index += 1;
                }
                Take::Open => return Some(param_index),
                Take::Mismatch if param.optional => param_index += 1,
                Take::Mismatch => return None,
            }
        }
        Some(param_index)
    }

    fn take(&self, param: &CommandParam, rest: &[&str]) -> Take {
        let token = rest[0];
        let listed = |values: &[Arc<str>]| {
            values.is_empty() || values.iter().any(|value| value.eq_ignore_ascii_case(token))
        };
        let matched = match &param.kind {
            CommandParamKind::Enum { values, .. } => listed(values),
            CommandParamKind::SoftEnum(name) => self
                .soft_enums
                .get(name)
                .is_none_or(|v| listed(v.as_slice())),
            CommandParamKind::Int => token.parse::<i64>().is_ok(),
            CommandParamKind::Float => token.parse::<f64>().is_ok(),
            CommandParamKind::Position => {
                return if rest.len() >= 3 {
                    Take::Took(3)
                } else {
                    Take::Open
                };
            }
            CommandParamKind::Rest => return Take::Open,
            CommandParamKind::Target | CommandParamKind::Word => true,
        };
        if matched {
            Take::Took(1)
        } else {
            Take::Mismatch
        }
    }

    fn param_values(
        &self,
        param: &CommandParam,
        partial: &str,
        context: CompletionContext<'_>,
        out: &mut Vec<Arc<str>>,
    ) {
        match &param.kind {
            CommandParamKind::Enum { values, .. } => extend_matching(out, values, partial),
            CommandParamKind::SoftEnum(name) => {
                if let Some(values) = self.soft_enums.get(name) {
                    extend_matching(out, values, partial);
                }
            }
            CommandParamKind::Target => {
                extend_players(out, context.players, partial, false);
                for selector in SELECTORS {
                    if contains_partial(selector, partial) {
                        out.push(Arc::from(selector));
                    }
                }
            }
            _ => {}
        }
    }
}

/// Returns whether a line enters plain-chat mention completion.
fn is_mention(before: &str) -> bool {
    !before.trim_start().starts_with('/') && before.contains('@')
}

/// Splits arguments on whitespace outside double quotes and retains the active token.
fn command_tokens(line: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (at, ch) in line.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        } else if ch.is_whitespace() && !quoted {
            if start < at {
                tokens.push(&line[start..at]);
            }
            start = at + ch.len_utf8();
        }
    }
    tokens.push(&line[start..]);
    tokens
}

/// Quotes player names with punctuation outside the command word alphabet or only digits.
pub fn quote_chat_player_name(name: &str) -> String {
    let word = name
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || b"()-.:?_".contains(&ch));
    if !word || name.bytes().all(|ch| ch.is_ascii_digit()) {
        format!("\"{name}\"")
    } else {
        name.to_owned()
    }
}

/// Adds matching roster names, retaining a mention marker for editor replacement.
fn extend_players(out: &mut Vec<Arc<str>>, players: &[Arc<str>], partial: &str, mention: bool) {
    for player in players {
        let name = quote_chat_player_name(player);
        if contains_partial(&name, partial) {
            out.push(Arc::from(if mention { format!("@{name}") } else { name }));
        }
    }
}

fn extend_matching(out: &mut Vec<Arc<str>>, values: &[Arc<str>], partial: &str) {
    out.extend(
        values
            .iter()
            .filter(|v| contains_partial(v, partial))
            .cloned(),
    );
}

fn visible(command: &CommandSpec, context: CompletionContext<'_>) -> bool {
    context
        .command_permission
        .is_none_or(|level| command.permission <= level)
}

/// Drops the already-typed token and clamps to the state limits; tree results
/// are also sorted case-insensitively and deduplicated.
fn finish(suggestions: &mut Vec<Arc<str>>, typed: &str, sort: bool) {
    if sort {
        suggestions.sort_by_cached_key(|value| value.to_ascii_lowercase());
        suggestions.dedup();
    }
    suggestions.retain(|value| &**value != typed);
    suggestions.truncate(MAX_CHAT_AUTOCOMPLETE);
    let mut bytes = 0usize;
    let keep = suggestions
        .iter()
        .take_while(|value| {
            bytes = bytes.saturating_add(value.len());
            bytes <= MAX_CHAT_AUTOCOMPLETE_BYTES
        })
        .count();
    suggestions.truncate(keep);
}

fn usage_of(command: &CommandSpec, overload: Option<&Arc<[CommandParam]>>) -> Arc<str> {
    let mut out = format!("/{}", command.name);
    for param in overload.map(|o| &**o).unwrap_or_default() {
        let label = match &param.kind {
            CommandParamKind::Enum { values, .. }
                if !param.collapse_enum
                    && !values.is_empty()
                    && values.len() <= MAX_USAGE_ENUM_VALUES =>
            {
                values.join("|")
            }
            _ => param.name.to_string(),
        };
        let _ = match (param.optional, &param.kind) {
            (false, CommandParamKind::Enum { values, .. }) if values.len() == 1 => {
                write!(out, " {label}")
            }
            (true, _) => write!(out, " [{label}]"),
            (false, _) => write!(out, " <{label}>"),
        };
    }
    Arc::from(out)
}

#[cfg(test)]
mod tests;
