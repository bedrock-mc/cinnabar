use super::{
    budget::MAX_RECORDS,
    model::Output,
    reader::{ReadResult, Reader, Refusal},
    screen::ScreenRecipeKind,
};

#[derive(Clone, Copy, Default)]
pub(super) struct Ingredient<'a> {
    pub(super) name: &'a str,
    pub(super) tag: bool,
    pub(super) aux: i32,
    pub(super) count: i32,
    pub(super) valid: bool,
}

pub(super) struct Candidate<'a> {
    pub(super) width: u8,
    pub(super) height: u8,
    pub(super) shapeless: bool,
    pub(super) mirror: bool,
    pub(super) priority: i32,
    pub(super) ingredients: [Ingredient<'a>; super::model::MAX_INGREDIENTS],
    pub(super) output: Output,
}

/// A screen recipe as read from the wire, borrowing its strings.
pub(super) struct ScreenRecord<'a> {
    pub(super) id: u32,
    pub(super) kind: ScreenRecipeKind,
    pub(super) ingredients: [Ingredient<'a>; 3],
    pub(super) len: usize,
    pub(super) output: Option<Output>,
}

/// Something the screens need beyond crafting-table recipes.
pub(super) enum ScreenItem<'a> {
    Recipe(ScreenRecord<'a>),
    Multi { uuid: [u8; 16], id: u32 },
}

pub(super) fn identifier(value: &str) -> bool {
    let Some((namespace, path)) = value.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !namespace.contains('/')
        && !path.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || matches!(b, b'_' | b'-' | b'.' | b'/' | b':')
        })
        && !path.contains(':')
}

fn ingredient<'a>(reader: &mut Reader<'a>) -> ReadResult<Ingredient<'a>> {
    let pairs = reader.count(8)?;
    let mut name = "";
    let mut tag = false;
    let mut valid = pairs == 1;
    for _ in 0..pairs {
        let key = reader.string()?;
        let value = reader.string()?;
        // `item_tag` descriptors name a tag rather than an item.
        match key {
            "name" | "item_tag" if identifier(value) => {
                name = value;
                tag = key == "item_tag";
            }
            _ => valid = false,
        }
    }
    let aux = reader.int()?;
    let count = reader.int()?;
    valid &= (0..=u16::MAX as i32).contains(&aux) && (1..=255).contains(&count);
    if pairs == 0 && count == 0 {
        valid = true;
    }
    Ok(Ingredient {
        name,
        tag,
        aux,
        count,
        valid,
    })
}

fn output(reader: &mut Reader<'_>) -> ReadResult<Option<Output>> {
    let id = reader.int()?;
    let bytes = reader.take(2)?;
    let count = u16::from_le_bytes([bytes[0], bytes[1]]);
    let aux = reader.uint()?;
    let block = reader.int()?;
    let extra = reader.opaque()?;
    // The existing validator is unchanged. The candidate accepts only the
    // independently parsed no-NBT/no-place/no-break envelope, or absent data.
    let empty = super::canonical_empty_extra(extra);
    // Block items carry negative network ids; only 0 (air) makes nothing.
    Ok(
        (id != 0 && (1..=255).contains(&count) && aux <= u16::MAX as u32 && empty).then_some(
            Output {
                id,
                aux: aux as u16,
                count: count as u8,
                // The recipe descriptor transports block identity as signed
                // ZigZag32; inventory item descriptors retain its raw u32 bits.
                // Hashed block identities may have bit 31 set.
                block: u32::from_ne_bytes(block.to_ne_bytes()),
                empty_envelope: !extra.is_empty(),
            },
        ),
    )
}

fn unlock(reader: &mut Reader<'_>) -> ReadResult<()> {
    if reader.byte()? == 0 {
        return Ok(());
    }
    // Discovery requirements belong to the recipe book/unlocked-recipe state,
    // not recipe admission. Native manual crafting checks unlock authority only
    // under the limited-crafting rules. Retain the recipe and traverse these
    // optional fields with the same bounds, without declaring it unlocked.
    reader.int()?;
    if reader.byte()? != 0 {
        let count = reader.count(64)?;
        for _ in 0..count {
            ingredient(reader)?;
        }
    }
    Ok(())
}

fn normal<'a>(
    reader: &mut Reader<'a>,
    shaped: bool,
    eligible: bool,
) -> ReadResult<(u32, Option<Candidate<'a>>, Option<ScreenRecord<'a>>)> {
    reader.string()?;
    let (width, height) = if shaped {
        (reader.int()?, reader.int()?)
    } else {
        (0, 0)
    };
    let count = reader.count(64)?;
    let mut ingredients = [Ingredient::default(); super::model::MAX_INGREDIENTS];
    let mut valid = eligible
        && if shaped {
            (1..=3).contains(&width)
                && (1..=3).contains(&height)
                && count == (width * height) as usize
        } else {
            (1..=super::model::MAX_INGREDIENTS).contains(&count)
        };
    for target in ingredients
        .iter_mut()
        .map(Some)
        .chain(std::iter::repeat_with(|| None))
        .take(count)
    {
        let item = ingredient(reader)?;
        // Shapeless recipes have no empty cells to describe.
        valid &= item.valid && (shaped || item.count > 0);
        if let Some(target) = target {
            *target = item;
        }
    }
    valid &= ingredients.iter().any(|i| i.count > 0);
    let results = reader.count(64)?;
    valid &= results == 1;
    let mut result = None;
    for _ in 0..results {
        let current = output(reader)?;
        if results == 1 {
            result = current;
        }
    }
    reader.take(16)?;
    let block = reader.string()?;
    let priority = reader.int()?;
    let mirror = shaped && reader.byte()? != 0;
    unlock(reader)?;
    let id = reader.uint()?;
    valid &= id != 0;
    let screen_kind = match block {
        "stonecutter" => Some(ScreenRecipeKind::Stonecutter),
        "cartography_table" => Some(ScreenRecipeKind::Cartography),
        "furnace" => Some(ScreenRecipeKind::Furnace),
        "blast_furnace" => Some(ScreenRecipeKind::BlastFurnace),
        "smoker" => Some(ScreenRecipeKind::Smoker),
        _ => None,
    };
    let screen = screen_kind
        .filter(|_| valid && !shaped)
        .zip(result)
        .map(|(kind, output)| {
            let mut kept = [Ingredient::default(); 3];
            let mut len = 0;
            for item in ingredients.iter().filter(|item| item.count > 0).take(3) {
                kept[len] = *item;
                len += 1;
            }
            ScreenRecord {
                id,
                kind,
                ingredients: kept,
                len,
                output: Some(output),
            }
        });
    valid &= block == "crafting_table";
    Ok((
        id,
        if valid {
            result.map(|output| Candidate {
                width: width as u8,
                height: height as u8,
                shapeless: !shaped,
                mirror,
                priority,
                ingredients,
                output,
            })
        } else {
            None
        },
        screen,
    ))
}

/// Traverses all eleven vectors, including unavailable families, without
/// generated owned/borrowed materialization. Returning Policy stops immediately;
/// it does not attest to the unseen tail's wire validity.
pub(super) fn walk<'a>(
    reader: &mut Reader<'a>,
    mut record: impl FnMut(u32, Option<Candidate<'a>>) -> ReadResult<()>,
    mut screen: impl FnMut(ScreenItem<'a>) -> ReadResult<()>,
) -> ReadResult<(bool, usize)> {
    let mut total = 0usize;
    for family in 0..11 {
        let count = reader.count(MAX_RECORDS)?;
        total = total.checked_add(count).ok_or(Refusal::Policy)?;
        if total > MAX_RECORDS {
            return Err(Refusal::Policy);
        }
        for _ in 0..count {
            match family {
                0 | 1 | 3 | 4 | 5 => {
                    let (id, candidate, screen_recipe) =
                        normal(reader, matches!(family, 0 | 5), matches!(family, 0 | 1))?;
                    if let Some(recipe) = screen_recipe {
                        screen(ScreenItem::Recipe(recipe))?;
                    }
                    record(id, candidate)?;
                }
                2 => {
                    let uuid: [u8; 16] = reader.take(16)?.try_into().map_err(|_| Refusal::Wire)?;
                    let id = reader.uint()?;
                    screen(ScreenItem::Multi { uuid, id })?;
                    record(id, None)?;
                }
                6 | 7 => {
                    reader.string()?;
                    let mut ingredients = [Ingredient::default(); 3];
                    for slot in &mut ingredients {
                        *slot = ingredient(reader)?;
                    }
                    let result = if family == 6 { output(reader)? } else { None };
                    reader.string()?;
                    let id = reader.uint()?;
                    let sound = ingredients.iter().all(|item| item.valid && item.count > 0)
                        && id != 0
                        && (family == 7 || result.is_some());
                    if sound {
                        screen(ScreenItem::Recipe(ScreenRecord {
                            id,
                            kind: if family == 6 {
                                ScreenRecipeKind::SmithingTransform
                            } else {
                                ScreenRecipeKind::SmithingTrim
                            },
                            ingredients,
                            len: 3,
                            output: result,
                        }))?;
                    }
                    record(id, None)?;
                }
                8 | 9 => {
                    for _ in 0..if family == 8 { 6 } else { 3 } {
                        reader.int()?;
                    }
                }
                10 => {
                    reader.int()?;
                    for _ in 0..reader.count(64)? {
                        reader.int()?;
                        reader.int()?;
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    let clear = reader.byte()? != 0;
    reader.finish()?;
    Ok((clear, total))
}
