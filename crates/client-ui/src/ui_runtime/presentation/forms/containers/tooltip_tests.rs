use super::tooltip_text;
use crate::ui_runtime::presentation::hud_layout::TooltipLine;

#[test]
fn tooltip_serializes_every_shared_palette_color_not_only_enchantments_and_lore() {
    for code in ('0'..='9').chain('a'..='v') {
        if matches!(code, 'k' | 'l' | 'o' | 'r') {
            continue;
        }
        let source = format!("§{code}Name");
        let parsed = ui::parse_bedrock_text(&source, source.len()).unwrap();
        let color = parsed[0].style.color;
        let [r, g, b] = color.rgb().unwrap_or([255; 3]);
        let encoded = tooltip_text(&[TooltipLine {
            text: "Name".into(),
            color: [r, g, b, 255],
        }])
        .unwrap();
        let decoded = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
        assert_eq!(decoded[0].style.color, color, "{code}: {encoded}");
        assert_eq!(decoded.plain_text(), "Name");
    }
}

#[test]
fn independent_line_styles_do_not_inherit_a_custom_names_bold_or_italic() {
    let gray = ui::BedrockColor::Gray.rgb().unwrap();
    let lines = [
        TooltipLine {
            text: "§l§oName".into(),
            color: [255; 4],
        },
        TooltipLine {
            text: "Enchantment".into(),
            color: [gray[0], gray[1], gray[2], 255],
        },
    ];
    let encoded = tooltip_text(&lines).unwrap();
    let parsed = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
    let enchant = parsed
        .iter()
        .find(|span| span.text.contains("Enchantment"))
        .unwrap();
    assert_eq!(enchant.style.color, ui::BedrockColor::Gray);
    assert!(!enchant.style.bold);
    assert!(!enchant.style.italic);
}

#[test]
fn server_authored_format_codes_still_override_the_line_base_color() {
    let aqua = ui::BedrockColor::Aqua.rgb().unwrap();
    let encoded = tooltip_text(&[TooltipLine {
        text: "§cServer Name".into(),
        color: [aqua[0], aqua[1], aqua[2], 255],
    }])
    .unwrap();
    let parsed = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
    assert_eq!(parsed[0].style.color, ui::BedrockColor::Red);
    assert!(tooltip_text(&[]).is_none());
}

#[test]
fn component_native_code_survives_when_legacy_rgb_does_not_match_the_ui_palette() {
    let encoded = tooltip_text(&[TooltipLine {
        text: "§vName".into(),
        color: [1, 2, 3, 255],
    }])
    .unwrap();
    let parsed = ui::parse_bedrock_text(&encoded, encoded.len()).unwrap();
    assert_eq!(parsed[0].style.color, ui::BedrockColor::MaterialResin);
}
