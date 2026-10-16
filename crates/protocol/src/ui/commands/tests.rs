use std::sync::Arc;

use bedrock_protocol_1_26_51::types::{
    Command, CommandEnum, CommandOverload, CommandParameter, DynamicEnum,
};

use super::*;

const VALID: u32 = 0x10_0000;

fn param(name: &str, word: u32, optional: bool) -> CommandParameter {
    CommandParameter {
        name: name.into(),
        parse_symbol: wire::U32LE(word),
        is_optional: optional,
        options: wire::U8(0),
    }
}

fn command(
    name: &str,
    permission: &str,
    alias_enum: i32,
    params: Vec<CommandParameter>,
) -> Command {
    Command {
        name: name.into(),
        description: wire::Str::default(),
        flags: wire::U16LE(0),
        permission_level: permission.into(),
        alias_enum: wire::I32LE(alias_enum),
        command_data_chained_subcommand_indexes: Vec::new(),
        overloads: vec![CommandOverload {
            is_chaining: false,
            parameter_data: params,
        }],
    }
}

fn catalog() -> ChatAutocompleteCatalog {
    let packet = AvailableCommands {
        enum_values: ["survival", "creative", "gm", "on", "off"]
            .map(wire::Str::from)
            .to_vec(),
        enum_data: vec![
            CommandEnum {
                name: "GameMode".into(),
                values: vec![wire::U32LE(0), wire::U32LE(1)],
            },
            CommandEnum {
                name: "gamemodeAliases".into(),
                values: vec![wire::U32LE(2)],
            },
            CommandEnum {
                name: "Toggle".into(),
                values: vec![wire::U32LE(3), wire::U32LE(4)],
            },
        ],
        commands: vec![
            command(
                "gamemode",
                "any",
                1,
                vec![
                    param("mode", ARG_ENUM | VALID, false),
                    param("player", 8 | VALID, true),
                ],
            ),
            command("give", "any", -1, vec![param("player", 8 | VALID, false)]),
            command("op", "admin", -1, vec![param("player", 8 | VALID, false)]),
            command(
                "warp",
                "any",
                -1,
                vec![
                    param("name", ARG_SOFT_ENUM | VALID, false),
                    param("speed", 3 | VALID, true),
                ],
            ),
            command(
                "flag",
                "any",
                -1,
                vec![
                    param("toggle", ARG_ENUM | VALID | 2, false),
                    param("at", 65 | VALID, false),
                    param("note", 67 | VALID, true),
                ],
            ),
        ],
        soft_enums: vec![DynamicEnum {
            enum_name: "warps".into(),
            enum_options: vec!["spawn".into(), "shop".into()],
        }],
        ..Default::default()
    };
    let UiEvent::AvailableCommands(event) = normalize_available_commands(packet) else {
        panic!("expected a command tree");
    };
    let mut catalog = ChatAutocompleteCatalog::default();
    catalog.apply_commands(event);
    catalog
}

#[test]
fn command_names_match_substrings_case_insensitively() {
    let catalog = catalog();
    let completion = catalog.complete("/MODE", 5).unwrap();
    assert_eq!(completion.suggestions.as_ref(), [Arc::from("/gamemode")]);
    let completion = catalog.complete("/iv", 3).unwrap();
    assert_eq!(completion.suggestions.as_ref(), [Arc::from("/give")]);
    assert!(
        catalog
            .complete("/unavailable", 12)
            .unwrap()
            .suggestions
            .is_empty()
    );
}

#[test]
fn command_name_highlights_use_offsets_in_the_displayed_suggestion() {
    let catalog = catalog();
    for (input, suggestion, range) in [
        ("/iv", "/give", 2..4),
        ("/IV", "/give", 2..4),
        ("/MODE", "/gamemode", 5..9),
        ("/g", "/give", 1..2),
        ("/m", "/gm", 2..3),
    ] {
        let completion = catalog.complete(input, input.len()).unwrap();
        assert!(
            completion
                .suggestions
                .iter()
                .any(|value| &**value == suggestion)
        );
        assert_eq!(
            ChatAutocompleteCatalog::suggestion_match(input, suggestion),
            Some(range)
        );
    }
    assert_eq!(
        ChatAutocompleteCatalog::suggestion_match("/", "/give"),
        None
    );
    assert_eq!(
        ChatAutocompleteCatalog::suggestion_match("/warp /s", "/spawn"),
        Some(0..2)
    );
}

fn suggest(catalog: &ChatAutocompleteCatalog, input: &str) -> Vec<String> {
    complete(catalog, input, CompletionContext::default()).0
}

fn complete(
    catalog: &ChatAutocompleteCatalog,
    input: &str,
    context: CompletionContext<'_>,
) -> (Vec<String>, Option<Arc<str>>) {
    let result = catalog.complete_in(input, input.len(), context).unwrap();
    (
        result.suggestions.iter().map(|s| s.to_string()).collect(),
        result.usage,
    )
}

#[test]
fn command_names_and_aliases_complete_with_the_slash() {
    let catalog = catalog();
    assert_eq!(
        suggest(&catalog, "/g"),
        ["/flag", "/gamemode", "/give", "/gm"]
    );
    assert_eq!(suggest(&catalog, "/m"), ["/gamemode", "/gm"]);
    assert_eq!(suggest(&catalog, "/GAME"), ["/gamemode"]);
    assert!(suggest(&catalog, "plain chat").is_empty());
}

#[test]
fn enum_values_complete_after_the_command() {
    let catalog = catalog();
    assert_eq!(suggest(&catalog, "/gamemode "), ["creative", "survival"]);
    assert_eq!(suggest(&catalog, "/gm su"), ["survival"]);
    assert_eq!(suggest(&catalog, "/gamemode survival "), SELECTORS);
}

#[test]
fn enum_values_match_substrings_case_insensitively() {
    let catalog = catalog();
    // "survival" contains "iva" mid-word; prefix-only filtering would miss it.
    assert_eq!(suggest(&catalog, "/gamemode iva"), ["survival"]);
    assert_eq!(suggest(&catalog, "/gamemode IVA"), ["survival"]);
    // Soft enums share the same substring rule ("shop" contains "hop").
    assert_eq!(suggest(&catalog, "/warp hop"), ["shop"]);
    assert_eq!(
        ChatAutocompleteCatalog::suggestion_match("/gamemode iva", "survival"),
        Some(4..7)
    );
}

#[test]
fn flat_enums_match_substrings_case_insensitively() {
    let mut catalog = ChatAutocompleteCatalog::default();
    catalog
        .apply(ChatAutocompleteEvent {
            enum_name: Arc::from("effects"),
            action: ChatAutocompleteAction::Replace,
            suggestions: Arc::from([Arc::from("night_vision"), Arc::from("speed")]),
        })
        .unwrap();
    assert_eq!(suggest(&catalog, "vis"), ["night_vision"]);
    assert_eq!(suggest(&catalog, "VIS"), ["night_vision"]);
}

#[test]
fn target_parameters_offer_players_and_selectors() {
    let catalog = catalog();
    let players = [Arc::from("Steve"), Arc::from("Alex")];
    let context = CompletionContext {
        players: &players,
        command_permission: None,
    };
    assert_eq!(
        complete(&catalog, "/give ", context).0[..2],
        ["@a", "@e"].map(String::from)[..2]
    );
    assert_eq!(complete(&catalog, "/give al", context).0, ["Alex"]);
}

#[test]
fn soft_enums_follow_updates() {
    let mut catalog = catalog();
    assert_eq!(suggest(&catalog, "/warp s"), ["shop", "spawn"]);
    catalog
        .apply(ChatAutocompleteEvent {
            enum_name: Arc::from("warps"),
            action: ChatAutocompleteAction::Add,
            suggestions: Arc::from([Arc::from("sky")]),
        })
        .unwrap();
    assert_eq!(suggest(&catalog, "/warp s"), ["shop", "sky", "spawn"]);
}

#[test]
fn permission_level_hides_commands_above_the_local_level() {
    let catalog = catalog();
    let low = CompletionContext {
        players: &[],
        command_permission: Some(0),
    };
    assert_eq!(complete(&catalog, "/o", low).0, ["/gamemode"]);
    assert_eq!(complete(&catalog, "/p", low).0, ["/warp"]);
    assert_eq!(complete(&catalog, "/op ", low), (Vec::new(), None));
    let admin = CompletionContext {
        command_permission: Some(2),
        ..low
    };
    assert_eq!(complete(&catalog, "/o", admin).0, ["/gamemode", "/op"]);
    assert_eq!(complete(&catalog, "/p", admin).0, ["/op", "/warp"]);
    assert_eq!(
        complete(&catalog, "/op ", admin).1.as_deref(),
        Some("/op <player>")
    );
}

#[test]
fn usage_lists_the_current_overload() {
    let catalog = catalog();
    let usage = |input: &str| complete(&catalog, input, CompletionContext::default()).1;
    assert_eq!(
        usage("/gamemode ").as_deref(),
        Some("/gamemode <survival|creative> [player]")
    );
    assert_eq!(usage("/warp ").as_deref(), Some("/warp <name> [speed]"));
    assert_eq!(usage("/nope "), None);
}

#[test]
fn contradicting_finished_tokens_yield_no_argument_suggestions() {
    let catalog = catalog();
    assert!(suggest(&catalog, "/gamemode bogus ").is_empty());
}

#[test]
fn positions_and_free_text_consume_their_tokens() {
    let catalog = catalog();
    assert_eq!(suggest(&catalog, "/flag "), ["off", "on"]);
    assert!(suggest(&catalog, "/flag on 1 2 ").is_empty());
    assert!(suggest(&catalog, "/flag on 1 2 3 hello wor").is_empty());
}

#[test]
fn already_typed_token_is_not_suggested_again() {
    let catalog = catalog();
    assert!(suggest(&catalog, "/give").is_empty());
}

#[test]
fn flat_enums_still_complete_without_a_command_tree() {
    let mut catalog = ChatAutocompleteCatalog::default();
    catalog
        .apply(ChatAutocompleteEvent {
            enum_name: Arc::from("commands"),
            action: ChatAutocompleteAction::Replace,
            suggestions: Arc::from([Arc::from("/give"), Arc::from("/gamemode")]),
        })
        .unwrap();
    assert_eq!(suggest(&catalog, "/g"), ["/give", "/gamemode"]);
}

#[test]
fn out_of_range_enum_index_degrades_to_a_word_parameter() {
    let mut packet = AvailableCommands::default();
    packet.commands.push(command(
        "x",
        "any",
        99,
        vec![param("a", ARG_ENUM | VALID | 7, false)],
    ));
    let UiEvent::AvailableCommands(event) = normalize_available_commands(packet) else {
        panic!("expected a command tree");
    };
    assert_eq!(
        event.commands[0].overloads[0][0].kind,
        CommandParamKind::Word
    );
    assert!(event.commands[0].aliases.is_empty());
}

#[test]
fn overlapping_enum_rows_share_pool_text_and_preserve_entries() {
    let packet = AvailableCommands {
        enum_values: vec![
            "first".into(),
            wire::Str::from(bytes::Bytes::from_static(b"bad\xff")),
            "unused".into(),
        ],
        enum_data: vec![
            CommandEnum {
                name: "one".into(),
                values: vec![
                    wire::U32LE(1),
                    wire::U32LE(0),
                    wire::U32LE(1),
                    wire::U32LE(99),
                ],
            },
            CommandEnum {
                name: "two".into(),
                values: vec![wire::U32LE(0), wire::U32LE(1)],
            },
        ],
        commands: vec![
            command(
                "one",
                "any",
                0,
                vec![param("value", ARG_ENUM | VALID, false)],
            ),
            command(
                "two",
                "any",
                1,
                vec![param("value", ARG_ENUM | VALID | 1, false)],
            ),
        ],
        ..Default::default()
    };
    let UiEvent::AvailableCommands(event) = normalize_available_commands(packet) else {
        panic!("expected a command tree");
    };
    let one = &event.commands[0].aliases;
    let two = &event.commands[1].aliases;
    assert_eq!(
        one.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
        ["bad\u{fffd}", "first", "bad\u{fffd}"]
    );
    assert_eq!(
        two.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
        ["first", "bad\u{fffd}"]
    );
    assert!(Arc::ptr_eq(&one[0], &one[2]));
    assert!(Arc::ptr_eq(&one[0], &two[1]));
    assert!(Arc::ptr_eq(&one[1], &two[0]));
    for command in event.commands.iter() {
        let CommandParamKind::Enum { values, .. } = &command.overloads[0][0].kind else {
            panic!("expected an enum parameter");
        };
        assert!(Arc::ptr_eq(values, &command.aliases));
    }
}

#[test]
fn plain_chat_mentions_offer_here_and_roster_names_without_commands() {
    let players = [
        Arc::from("Steve"),
        Arc::from("Alex"),
        Arc::from("Alice One"),
    ];
    let context = CompletionContext {
        players: &players,
        command_permission: Some(0),
    };
    for catalog in [ChatAutocompleteCatalog::default(), catalog()] {
        let (suggestions, usage) = complete(&catalog, "hello @", context);
        assert_eq!(suggestions, ["@\"Alice One\"", "@here", "@Alex", "@Steve"]);
        assert_eq!(usage, None);
        assert_eq!(complete(&catalog, "@h", context).0, [ALL_PLAYERS_MENTION]);
        assert!(complete(&catalog, "@everyone", context).0.is_empty());
        assert!(complete(&catalog, "@a[", context).0.is_empty());
        assert!(complete(&catalog, "hello", context).0.is_empty());
    }
}

#[test]
fn completed_mentions_are_removed_without_hiding_longer_matches() {
    let players = [
        Arc::from("Alex"),
        Arc::from("AlexTwo"),
        Arc::from("Alice One"),
        Arc::from("hereafter"),
    ];
    let context = CompletionContext {
        players: &players,
        command_permission: None,
    };
    for catalog in [ChatAutocompleteCatalog::default(), catalog()] {
        assert_eq!(complete(&catalog, "hello @here", context).0, ["@hereafter"]);
        assert_eq!(complete(&catalog, "hello @Alex", context).0, ["@AlexTwo"]);
        assert!(
            complete(&catalog, "hello @\"Alice One\"", context)
                .0
                .is_empty()
        );
    }
}

#[test]
fn mention_filtering_is_case_insensitive_and_prefers_prefix_matches() {
    let players = [Arc::from("Malex"), Arc::from("alex"), Arc::from("Alex")];
    let context = CompletionContext {
        players: &players,
        command_permission: None,
    };
    assert_eq!(
        complete(&catalog(), "@AL", context).0,
        ["@Alex", "@alex", "@Malex"]
    );
    assert_eq!(
        complete(&catalog(), "@unknown then @lex", context).0,
        ["@Alex", "@Malex", "@alex"]
    );
    assert_eq!(
        ChatAutocompleteCatalog::replacement_start("mail@example @Al"),
        13
    );
}

#[test]
fn player_names_are_quoted_and_remain_one_command_argument() {
    let players = [
        Arc::from("Alice One"),
        Arc::from("123"),
        Arc::from("Alex_One"),
    ];
    let context = CompletionContext {
        players: &players,
        command_permission: None,
    };
    assert_eq!(
        complete(&catalog(), "/give one", context).0,
        ["\"Alice One\"", "Alex_One"]
    );
    assert_eq!(complete(&catalog(), "/give 12", context).0, ["\"123\""]);
    assert_eq!(
        complete(&catalog(), "/give \"Alice O", context).0,
        ["\"Alice One\""]
    );
    assert_eq!(
        complete(&catalog(), "@Alice O", context).0,
        ["@\"Alice One\""]
    );
    assert!(
        complete(&catalog(), "/give \"Alice One\" ", context)
            .0
            .is_empty()
    );
    assert_eq!(
        ChatAutocompleteCatalog::replacement_start("/give \"Alice O"),
        6
    );
}

#[test]
fn mentions_are_independent_of_command_permission() {
    for permission in [None, Some(0), Some(1), Some(2), Some(5)] {
        let context = CompletionContext {
            players: &[],
            command_permission: permission,
        };
        assert_eq!(complete(&catalog(), "@", context).0, [ALL_PLAYERS_MENTION]);
    }
}

#[test]
fn selectors_filter_against_one_catalog_and_have_matching_description_keys() {
    let catalog = catalog();
    assert_eq!(suggest(&catalog, "/give @"), SELECTORS);
    for selector in SELECTORS {
        let input = format!("/give {}", selector.to_ascii_uppercase());
        assert_eq!(suggest(&catalog, &input), [selector]);
        assert_eq!(
            ChatAutocompleteCatalog::suggestion_description(&input, selector),
            Some(format!("commands.autocomplete.{}", &selector[1..]))
        );
    }
    assert_eq!(suggest(&catalog, "/give n"), ["@n"]);
    assert!(suggest(&catalog, "/give @here").is_empty());
    assert!(suggest(&catalog, "/give @initiator").is_empty());
    assert!(suggest(&catalog, "/give @c").is_empty());
    assert!(suggest(&catalog, "/give @v").is_empty());
}

#[test]
fn mention_results_remain_bounded() {
    let players: Vec<_> = (0..MAX_CHAT_AUTOCOMPLETE * 2)
        .map(|index| Arc::from(format!("Player{index}")))
        .collect();
    let context = CompletionContext {
        players: &players,
        command_permission: None,
    };
    let completion = complete(&catalog(), "@", context).0;
    assert_eq!(completion.len(), MAX_CHAT_AUTOCOMPLETE);
    assert!(completion.iter().map(String::len).sum::<usize>() <= MAX_CHAT_AUTOCOMPLETE_BYTES);
}
