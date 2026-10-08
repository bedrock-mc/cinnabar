use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::{
    AvailableCommandsPacket, AvailableCommandsPacketCommandDatajson,
    AvailableCommandsPacketPayloadEnumData, AvailableCommandsPacketPayloadOverloadData,
    AvailableCommandsPacketPayloadParamData, AvailableCommandsPacketPayloadSoftEnumData,
};

use super::*;

const VALID: u32 = 0x10_0000;

fn param(name: &str, word: u32, optional: bool) -> AvailableCommandsPacketPayloadParamData {
    AvailableCommandsPacketPayloadParamData {
        name: name.to_owned(),
        parse_symbol: word,
        is_optional: optional,
        options: 0,
    }
}

fn command(
    name: &str,
    permission: &str,
    alias_enum: i32,
    params: Vec<AvailableCommandsPacketPayloadParamData>,
) -> AvailableCommandsPacketCommandDatajson {
    AvailableCommandsPacketCommandDatajson {
        name: name.to_owned(),
        description: String::new(),
        flags: 0,
        permission_level: permission.to_owned(),
        alias_enum,
        command_data_chained_subcommand_indexes: Vec::new(),
        overloads: vec![AvailableCommandsPacketPayloadOverloadData {
            is_chaining: false,
            parameter_data: params,
        }],
    }
}

fn catalog() -> ChatAutocompleteCatalog {
    let packet = AvailableCommandsPacket {
        enum_values: ["survival", "creative", "gm", "on", "off"]
            .map(str::to_owned)
            .to_vec(),
        enum_data: vec![
            AvailableCommandsPacketPayloadEnumData {
                name: "GameMode".to_owned(),
                values: vec![0, 1],
            },
            AvailableCommandsPacketPayloadEnumData {
                name: "gamemodeAliases".to_owned(),
                values: vec![2],
            },
            AvailableCommandsPacketPayloadEnumData {
                name: "Toggle".to_owned(),
                values: vec![3, 4],
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
        soft_enums: vec![AvailableCommandsPacketPayloadSoftEnumData {
            enum_name: "warps".to_owned(),
            enum_options: vec!["spawn".to_owned(), "shop".to_owned()],
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
    assert_eq!(
        suggest(&catalog, "/gamemode survival "),
        ["@a", "@e", "@p", "@r", "@s"]
    );
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
    let mut packet = AvailableCommandsPacket::default();
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
