use super::{BedrockColor, FORMATTING_COLORS, FormattingPalette, parse_bedrock_text};
use std::collections::BTreeSet;

#[test]
fn descriptors_cover_the_colour_inventory_without_duplicate_names_or_codes() {
    let mut colors = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut codes = BTreeSet::new();
    let mut globals = BTreeSet::new();
    for descriptor in FORMATTING_COLORS {
        assert!(colors.insert(descriptor.color as usize));
        if let Some(name) = descriptor.name {
            assert!(names.insert(name));
            assert_eq!(BedrockColor::from_name(name), Some(descriptor.color));
        }
        assert!(codes.insert(descriptor.code));
        assert!(globals.insert(descriptor.global));
        let text = format!("§{}sample", descriptor.code);
        let spans = parse_bedrock_text(&text, text.len()).unwrap();
        assert_eq!(spans[0].style.color, descriptor.color);
    }
    // The semantic enum is the inventory, independent of the descriptor table.
    let inventory = (0..=BedrockColor::PartyBlue as usize)
        .filter(|index| *index != BedrockColor::Base as usize)
        .collect();
    assert_eq!(colors, inventory);
    assert_eq!(BedrockColor::Base.descriptor(), None);
}

#[test]
fn component_colours_follow_active_palette_replacements() {
    let color = BedrockColor::from_name("MATERIAL_EMERALD").unwrap();
    assert_eq!(color, BedrockColor::MaterialEmerald);
    assert_eq!(FormattingPalette::default().rgb(color), color.rgb());
    let palette = FormattingPalette::from_globals(|name| {
        (name == "$material_emerald_color").then_some([0.25, 0.5, 0.75])
    });
    assert_eq!(palette.rgb(color), Some([64, 128, 191]));
    assert_eq!(palette.rgb(BedrockColor::Base), None);
    assert_eq!(BedrockColor::from_name("unrecognized"), None);
    assert_eq!(BedrockColor::from_code('Q'), None);
}
