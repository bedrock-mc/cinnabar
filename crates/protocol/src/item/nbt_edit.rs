//! Rewrites a few root tags of an item's user data, keeping every other tag and the trailer.
//!
//! The user-data layout is described on [`super::decode_extra_nbt`]: an `int16` marker, a
//! version byte and one fixed little-endian compound when the marker is `-1`, followed by the
//! `canPlaceOn` / `canBreak` lists (and a shield's blocking tick), which are copied unchanged.

use std::sync::Arc;

use super::{
    compound_field, decode_extra_nbt, read_i32_le, read_u8, read_u16_le, root_tag, skip_le_payload,
};

const TAG_SHORT: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;

/// User data with no compound and empty `canPlaceOn` / `canBreak` lists.
pub(super) const EMPTY_USER_DATA: [u8; 10] = [0; 10];

/// Item tags changed by a local edit or accepted response. `None` leaves a tag as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemNbtEdit {
    /// The root `Damage` int.
    pub damage: Option<i32>,
    /// The root `RepairCost` int: the prior-work penalty anvils charge.
    pub repair_cost: Option<i32>,
    /// The root `ench` list as `(id, level)` pairs; an empty list removes the tag.
    pub enchantments: Option<Vec<(i16, i16)>>,
    /// `display.Name`: `Some(None)` removes a custom name.
    pub custom_name: Option<Option<String>>,
    /// `display.FilteredName`; a new unfiltered name removes the old filtered value.
    pub filtered_custom_name: Option<Option<String>>,
}

/// Item properties used for anvil edits, without tooltip limits or level conversion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemAnvilData {
    pub custom_name: Option<Arc<str>>,
    pub enchantments: Vec<(i16, i16)>,
}

/// Reads the full custom name without tooltip limits, independently of enchantment support.
#[must_use]
pub fn item_custom_name(extra_data: &[u8]) -> Option<Arc<str>> {
    display_string(extra_data, b"Name")
}

/// Reads the optional filtered custom name from the retained display compound.
#[must_use]
pub fn item_filtered_custom_name(extra_data: &[u8]) -> Option<Arc<str>> {
    display_string(extra_data, b"FilteredName")
}

/// Borrows a display string before allocating only its returned text.
fn display_string(extra_data: &[u8], name: &[u8]) -> Option<Arc<str>> {
    let mut nbt = decode_extra_nbt(extra_data)?;
    let mut display = root_tag(&mut nbt, TAG_COMPOUND, b"display")?;
    let mut text = compound_field(&mut display, TAG_STRING, name)?;
    let length = usize::from(read_u16_le(&mut text)?);
    Some(Arc::from(std::str::from_utf8(text.get(..length)?).ok()?))
}

/// Reads complete anvil inputs. Unsupported enchantment fields or name encodings return
/// `None` so prediction cannot discard data when rebuilding a changed enchantment list.
#[must_use]
pub fn item_anvil_data(extra_data: &[u8]) -> Option<ItemAnvilData> {
    let (_, entries, _) = read_extra_entries(extra_data)?;
    let mut data = ItemAnvilData::default();
    let mut saw_enchantments = false;
    let mut saw_display = false;
    for entry in &entries {
        match entry.name.as_slice() {
            b"ench" => {
                if saw_enchantments || entry.tag != TAG_LIST {
                    return None;
                }
                saw_enchantments = true;
                let mut list = entry.payload.as_slice();
                let element_type = read_u8(&mut list)?;
                let count = usize::try_from(read_i32_le(&mut list)?).ok()?;
                if count > 0 && element_type != TAG_COMPOUND {
                    return None;
                }
                for _ in 0..count {
                    let (fields, rest) = read_entries(list)?;
                    list = rest;
                    let (mut id, mut level) = (None, None);
                    for field in fields {
                        if field.tag != TAG_SHORT {
                            return None;
                        }
                        let target = match field.name.as_slice() {
                            b"id" => &mut id,
                            b"lvl" => &mut level,
                            _ => return None,
                        };
                        if target.is_some() {
                            return None;
                        }
                        *target = Some(i16::from_le_bytes(
                            field.payload.as_slice().try_into().ok()?,
                        ));
                    }
                    data.enchantments.push((id?, level?));
                }
            }
            b"display" => {
                if saw_display || entry.tag != TAG_COMPOUND {
                    return None;
                }
                saw_display = true;
                let (fields, _) = read_entries(&entry.payload)?;
                for field in fields.into_iter().filter(|field| field.name == b"Name") {
                    if data.custom_name.is_some() || field.tag != TAG_STRING {
                        return None;
                    }
                    let mut text = field.payload.as_slice();
                    let length = usize::from(read_u16_le(&mut text)?);
                    data.custom_name =
                        Some(Arc::from(std::str::from_utf8(text.get(..length)?).ok()?));
                }
            }
            _ => {}
        }
    }
    Some(data)
}

/// The root `RepairCost` of a stack's user data, `0` when absent or malformed.
#[must_use]
pub fn item_repair_cost(extra_data: &[u8]) -> i32 {
    decode_extra_nbt(extra_data)
        .and_then(|nbt| {
            let mut cursor = nbt;
            let mut payload = root_tag(&mut cursor, TAG_INT, b"RepairCost")?;
            read_i32_le(&mut payload)
        })
        .unwrap_or(0)
}

/// One named tag of a compound, kept as its encoded payload.
struct Entry {
    tag: u8,
    name: Vec<u8>,
    payload: Vec<u8>,
}

/// Splits a compound body (after its own tag and name) into entries and returns the rest.
fn read_entries(mut cursor: &[u8]) -> Option<(Vec<Entry>, &[u8])> {
    let mut entries = Vec::new();
    loop {
        let tag = read_u8(&mut cursor)?;
        if tag == 0 {
            return Some((entries, cursor));
        }
        let name_len = usize::from(read_u16_le(&mut cursor)?);
        let name = cursor.get(..name_len)?.to_vec();
        cursor = cursor.get(name_len..)?;
        let start = cursor;
        skip_le_payload(&mut cursor, tag, 1)?;
        let payload = start.get(..start.len() - cursor.len())?.to_vec();
        entries.push(Entry { tag, name, payload });
    }
}

/// Writes a little-endian length-prefixed string; `None` when it is too long for its prefix.
fn write_string(out: &mut Vec<u8>, text: &[u8]) -> Option<()> {
    out.extend_from_slice(&u16::try_from(text.len()).ok()?.to_le_bytes());
    out.extend_from_slice(text);
    Some(())
}

/// Writes a compound body: each entry's tag, name and payload, then the end tag.
fn write_entries(out: &mut Vec<u8>, entries: &[Entry]) -> Option<()> {
    for entry in entries {
        out.push(entry.tag);
        write_string(out, &entry.name)?;
        out.extend_from_slice(&entry.payload);
    }
    out.push(0);
    Some(())
}

/// Replaces the entry named `name`, or appends it; `None` removes it.
fn set_entry(entries: &mut Vec<Entry>, name: &[u8], value: Option<(u8, Vec<u8>)>) {
    let position = entries.iter().position(|entry| entry.name == name);
    match (position, value) {
        (Some(index), Some((tag, payload))) => {
            entries[index] = Entry {
                tag,
                name: name.to_vec(),
                payload,
            }
        }
        (None, Some((tag, payload))) => entries.push(Entry {
            tag,
            name: name.to_vec(),
            payload,
        }),
        (Some(index), None) => {
            entries.remove(index);
        }
        (None, None) => {}
    }
}

/// The payload of an `ench` list: one compound of `id` and `lvl` shorts per enchantment.
fn enchantment_list(enchantments: &[(i16, i16)]) -> Option<Vec<u8>> {
    let mut payload = vec![TAG_COMPOUND];
    payload.extend_from_slice(&i32::try_from(enchantments.len()).ok()?.to_le_bytes());
    for (id, level) in enchantments {
        for (name, value) in [(&b"id"[..], id), (&b"lvl"[..], level)] {
            payload.push(TAG_SHORT);
            write_string(&mut payload, name)?;
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(0);
    }
    Some(payload)
}

/// Applies `edit` to a stack's user data. Unknown root tags, other `display` entries and the
/// trailing lists are kept byte for byte. `None` when the user data is malformed.
#[must_use]
pub fn edit_item_extra(extra_data: &[u8], edit: &ItemNbtEdit) -> Option<Arc<[u8]>> {
    let (root_name, mut entries, trailer) = read_extra_entries(extra_data)?;
    if let Some(damage) = edit.damage {
        set_entry(
            &mut entries,
            b"Damage",
            Some((TAG_INT, damage.to_le_bytes().to_vec())),
        );
    }
    if let Some(cost) = edit.repair_cost {
        set_entry(
            &mut entries,
            b"RepairCost",
            Some((TAG_INT, cost.to_le_bytes().to_vec())),
        );
    }
    if let Some(enchantments) = &edit.enchantments {
        let list = (!enchantments.is_empty())
            .then(|| enchantment_list(enchantments))
            .flatten()
            .map(|payload| (TAG_LIST, payload));
        set_entry(&mut entries, b"ench", list);
    }
    if edit.custom_name.is_some() || edit.filtered_custom_name.is_some() {
        let mut display = match entries.iter().find(|entry| entry.name == b"display") {
            Some(entry) if entry.tag == TAG_COMPOUND => read_entries(&entry.payload)?.0,
            _ => Vec::new(),
        };
        let remove_filtered = None;
        let filtered_name = edit
            .filtered_custom_name
            .as_ref()
            .or_else(|| edit.custom_name.as_ref().map(|_| &remove_filtered));
        for (key, value) in [
            (&b"Name"[..], edit.custom_name.as_ref()),
            (&b"FilteredName"[..], filtered_name),
        ] {
            let Some(value) = value else { continue };
            let value = match value {
                Some(text) => {
                    let mut payload = Vec::new();
                    write_string(&mut payload, text.as_bytes())?;
                    Some((TAG_STRING, payload))
                }
                None => None,
            };
            set_entry(&mut display, key, value);
        }
        let payload = if display.is_empty() {
            None
        } else {
            let mut payload = Vec::new();
            write_entries(&mut payload, &display)?;
            Some((TAG_COMPOUND, payload))
        };
        set_entry(&mut entries, b"display", payload);
    }
    let mut out = Vec::with_capacity(extra_data.len() + 64);
    if entries.is_empty() {
        out.extend_from_slice(&0i16.to_le_bytes());
    } else {
        out.extend_from_slice(&(-1i16).to_le_bytes());
        out.push(1);
        out.push(TAG_COMPOUND);
        write_string(&mut out, &root_name)?;
        write_entries(&mut out, &entries)?;
    }
    out.extend_from_slice(trailer);
    super::super::inventory::validate_item_nbt_size(out.len()).ok()?;
    Some(Arc::from(out))
}

/// Whether a valid writable book has no user-data compound yet.
/// Existing user data is kept even when it has no `pages` list.
#[must_use]
pub fn book_needs_init(extra_data: &[u8]) -> bool {
    read_extra_entries(extra_data).is_some() && decode_extra_nbt(extra_data).is_none()
}

/// Builds user data with an empty root `pages` list, preserving every other
/// root tag and the trailing lists. `None` when the stack already carries
/// `pages` or its user data is malformed.
#[must_use]
pub fn book_init_extra(extra_data: &[u8]) -> Option<Arc<[u8]>> {
    let (root_name, mut entries, trailer) = read_extra_entries(extra_data)?;
    if entries
        .iter()
        .any(|entry| entry.tag == TAG_LIST && entry.name == b"pages")
    {
        return None;
    }
    let mut payload = vec![TAG_COMPOUND];
    payload.extend_from_slice(&0i32.to_le_bytes());
    set_entry(&mut entries, b"pages", Some((TAG_LIST, payload)));
    let mut out = Vec::with_capacity(extra_data.len() + 64);
    out.extend_from_slice(&(-1i16).to_le_bytes());
    out.push(1);
    out.push(TAG_COMPOUND);
    write_string(&mut out, &root_name)?;
    write_entries(&mut out, &entries)?;
    out.extend_from_slice(trailer);
    super::super::inventory::validate_item_nbt_size(out.len()).ok()?;
    Some(Arc::from(out))
}

/// Splits user data into its root name, compound entries and unchanged trailer.
fn read_extra_entries(extra_data: &[u8]) -> Option<(Vec<u8>, Vec<Entry>, &[u8])> {
    let extra = if extra_data.is_empty() {
        &EMPTY_USER_DATA[..]
    } else {
        extra_data
    };
    let marker = i16::from_le_bytes(extra.get(..2)?.try_into().ok()?);
    match marker {
        -1 => {
            if *extra.get(2)? != 1 {
                return None;
            }
            let mut cursor = extra.get(3..)?;
            if read_u8(&mut cursor)? != TAG_COMPOUND {
                return None;
            }
            let name_len = usize::from(read_u16_le(&mut cursor)?);
            let name = cursor.get(..name_len)?.to_vec();
            let (entries, trailer) = read_entries(cursor.get(name_len..)?)?;
            Some((name, entries, trailer))
        }
        0 => Some((Vec::new(), Vec::new(), extra.get(2..)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{item_display, item_extra_damage};

    /// A sword with a custom name and lore, damage 7, and a trailing blocking tick.
    fn named_sword() -> Vec<u8> {
        let mut root = Vec::new();
        root.push(TAG_COMPOUND);
        write_string(&mut root, b"display").unwrap();
        let mut display = vec![TAG_STRING];
        write_string(&mut display, b"Name").unwrap();
        write_string(&mut display, b"Old").unwrap();
        display.push(TAG_LIST);
        write_string(&mut display, b"Lore").unwrap();
        display.push(TAG_STRING);
        display.extend_from_slice(&1i32.to_le_bytes());
        write_string(&mut display, b"kept lore").unwrap();
        display.push(0);
        root.extend_from_slice(&display);
        root.push(TAG_INT);
        write_string(&mut root, b"Damage").unwrap();
        root.extend_from_slice(&7i32.to_le_bytes());
        root.push(0);
        let mut extra = vec![0xff, 0xff, 1, TAG_COMPOUND, 0, 0];
        extra.extend_from_slice(&root);
        extra.extend_from_slice(&[0; 8]);
        extra.extend_from_slice(&42i64.to_le_bytes());
        extra
    }

    #[test]
    fn empty_enchantment_lists_accept_end_element_type() {
        for element_type in [0, TAG_COMPOUND] {
            let mut extra = vec![0xff, 0xff, 1, TAG_COMPOUND, 0, 0, TAG_LIST];
            write_string(&mut extra, b"ench").unwrap();
            extra.push(element_type);
            extra.extend_from_slice(&0i32.to_le_bytes());
            extra.push(0);
            extra.extend_from_slice(&[0; 8]);
            assert_eq!(item_anvil_data(&extra), Some(ItemAnvilData::default()));
            let renamed = edit_item_extra(
                &extra,
                &ItemNbtEdit {
                    custom_name: Some(Some("Blade".into())),
                    ..Default::default()
                },
            )
            .unwrap();
            let data = item_anvil_data(&renamed).unwrap();
            assert_eq!(data.custom_name.as_deref(), Some("Blade"));
            assert!(data.enchantments.is_empty());
        }
    }

    #[test]
    fn edits_replace_root_tags_and_keep_the_rest() {
        let extra = named_sword();
        let edited = edit_item_extra(
            &extra,
            &ItemNbtEdit {
                damage: Some(2),
                repair_cost: Some(3),
                enchantments: Some(vec![(9, 4), (17, 1)]),
                custom_name: Some(Some("New".into())),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(item_extra_damage(&edited), Some(2));
        assert_eq!(item_repair_cost(&edited), 3);
        let display = item_display(&edited);
        assert_eq!(display.name.as_deref(), Some("New"));
        assert_eq!(display.lore.len(), 1);
        assert_eq!(display.enchantments, vec![(9, 4), (17, 1)]);
        assert!(
            edited.ends_with(&42i64.to_le_bytes()),
            "the trailer survives"
        );
    }

    #[test]
    fn removing_the_name_keeps_lore_and_unchanged_edits_are_lossless() {
        let extra = named_sword();
        let unnamed = edit_item_extra(
            &extra,
            &ItemNbtEdit {
                custom_name: Some(None),
                ..ItemNbtEdit::default()
            },
        )
        .unwrap();
        let display = item_display(&unnamed);
        assert_eq!(display.name, None);
        assert_eq!(display.lore.len(), 1);
        assert_eq!(
            &edit_item_extra(&extra, &ItemNbtEdit::default()).unwrap()[..],
            &extra[..]
        );
    }

    #[test]
    fn plain_stacks_gain_a_compound() {
        let edited = edit_item_extra(
            &[],
            &ItemNbtEdit {
                repair_cost: Some(1),
                ..ItemNbtEdit::default()
            },
        )
        .unwrap();
        assert_eq!(item_repair_cost(&edited), 1);
        assert!(edited.ends_with(&[0; 8]));
        assert_eq!(item_repair_cost(&[]), 0);
        assert_eq!(edit_item_extra(&[5, 0], &ItemNbtEdit::default()), None);
    }

    #[test]
    fn anvil_data_retains_all_enchantments_and_full_short_levels() {
        let enchantments: Vec<_> = (0..40).map(|id| (id, 300)).collect();
        let extra = edit_item_extra(
            &[],
            &ItemNbtEdit {
                enchantments: Some(enchantments.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(item_anvil_data(&extra).unwrap().enchantments, enchantments);
        assert_eq!(item_anvil_data(&[]), Some(ItemAnvilData::default()));
        assert_eq!(item_anvil_data(&[5, 0]), None);
    }

    #[test]
    fn anvil_data_reads_names_beyond_the_tooltip_limit() {
        let name = "Blade".repeat(300);
        let extra = edit_item_extra(
            &[],
            &ItemNbtEdit {
                custom_name: Some(Some(name.clone())),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            item_anvil_data(&extra).unwrap().custom_name.as_deref(),
            Some(name.as_str())
        );
        assert_eq!(item_custom_name(&extra).as_deref(), Some(name.as_str()));
    }

    #[test]
    fn anvil_data_declines_enchantment_fields_the_editor_cannot_keep() {
        let mut list = enchantment_list(&[(9, 1)]).unwrap();
        let end = list.len() - 1;
        let mut field = vec![TAG_SHORT];
        write_string(&mut field, b"custom").unwrap();
        field.extend_from_slice(&42i16.to_le_bytes());
        list.splice(end..end, field);
        let mut extra = vec![0xff, 0xff, 1, TAG_COMPOUND, 0, 0];
        write_entries(
            &mut extra,
            &[Entry {
                tag: TAG_LIST,
                name: b"ench".to_vec(),
                payload: list,
            }],
        )
        .unwrap();
        extra.extend_from_slice(&[0; 8]);
        assert_eq!(item_anvil_data(&extra), None);
        assert_eq!(item_display(&extra).enchantments, vec![(9, 1)]);
        assert_eq!(
            edit_item_extra(&extra, &ItemNbtEdit::default())
                .unwrap()
                .as_ref(),
            extra.as_slice()
        );
    }

    #[test]
    fn fresh_books_need_init_and_gain_an_empty_pages_list() {
        assert!(book_needs_init(&[]));
        let init = book_init_extra(&[]).expect("empty book inits");
        assert!(!book_needs_init(&init));
        assert!(book_init_extra(&init).is_none());
        assert_eq!(crate::item_book(&init).pages.len(), 0);
        assert!(init.ends_with(&[0; 8]));
        assert!(!book_needs_init(&[5, 0]));
        assert!(book_init_extra(&[5, 0]).is_none());
    }

    #[test]
    fn book_use_preserves_existing_user_data_without_pages() {
        let named = edit_item_extra(
            &[],
            &ItemNbtEdit {
                custom_name: Some(Some("Field notes".to_owned())),
                ..ItemNbtEdit::default()
            },
        )
        .unwrap();
        assert!(crate::item_book(&named).pages.is_empty());
        assert!(!book_needs_init(&named));
    }
}
