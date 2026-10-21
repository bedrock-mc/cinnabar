//! Bounded filename-to-raw-sprite bindings for resource-pack inheritance.

use super::*;

/// One normalized source filename resolving to its unprocessed sprite pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IconSource {
    pub path: Box<str>,
    pub sprite: u32,
}

/// Rejects aliases that cannot identify one canonical admitted raster.
fn validate(sources: &[IconSource], sprites: usize) -> Result<(), AssetError> {
    if sources.len() > MAX_ICON_SPRITES {
        return Err(invalid("icon source count exceeds bound"));
    }
    let mut previous = None;
    for source in sources {
        let path = source.path.as_ref();
        if path.is_empty()
            || path.len() > crate::entity::MAX_ENTITY_ASSET_PATH_BYTES
            || !path.starts_with("textures/")
            || !(path.ends_with(".png") || path.ends_with(".tga"))
            || path
                .bytes()
                .any(|byte| byte.is_ascii_uppercase() || byte == b'\\')
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
            || source.sprite as usize >= sprites
            || previous.is_some_and(|previous| previous >= path)
        {
            return Err(invalid("noncanonical icon source binding"));
        }
        previous = Some(path);
    }
    Ok(())
}

/// Appends validated source bindings within the carrier's shared payload budget.
pub(super) fn encode(
    payload: &mut Vec<u8>,
    sources: &[IconSource],
    sprites: usize,
) -> Result<(), AssetError> {
    validate(sources, sprites)?;
    let mut table = Vec::new();
    table.extend_from_slice(&(sources.len() as u32).to_le_bytes());
    for source in sources {
        table.extend_from_slice(&(source.path.len() as u16).to_le_bytes());
        table.extend_from_slice(source.path.as_bytes());
        table.extend_from_slice(&source.sprite.to_le_bytes());
    }
    crate::encoding::append_bounded(
        payload,
        &table,
        MAX_ICON_CARRIER_BYTES,
        HEADER_BYTES + HASH_BYTES,
    )
    .ok_or_else(|| invalid("icon carrier exceeds bound"))
}

/// Reads sealed source bindings without allowing a string to escape the payload.
pub(super) fn decode(
    bytes: &[u8],
    cursor: &mut usize,
    end: usize,
    sprites: usize,
) -> Result<Vec<IconSource>, AssetError> {
    let count = read_u32(bytes, *cursor)? as usize;
    *cursor += 4;
    if count > MAX_ICON_SPRITES {
        return Err(invalid("icon source count exceeds bound"));
    }
    let mut sources = Vec::with_capacity(count);
    for _ in 0..count {
        let length = usize::from(read_u16(bytes, *cursor)?);
        *cursor += 2;
        let path_end = cursor
            .checked_add(length)
            .filter(|path_end| path_end.checked_add(4).is_some_and(|next| next <= end))
            .ok_or_else(|| invalid("icon source runs past the payload"))?;
        let path = std::str::from_utf8(&bytes[*cursor..path_end])
            .map_err(|_| invalid("icon source is not UTF-8"))?;
        let sprite = read_u32(bytes, path_end)?;
        *cursor = path_end + 4;
        sources.push(IconSource {
            path: path.into(),
            sprite,
        });
    }
    validate(&sources, sprites)?;
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_source_pixels_stay_distinct_from_processed_item_thumbnails() {
        let sprites = [
            IconSprite {
                width: 1,
                height: 1,
                rgba8: Arc::from([12, 34, 56, 255]),
            },
            IconSprite {
                width: 1,
                height: 1,
                rgba8: Arc::from([200, 210, 220, 96]),
            },
        ];
        let sources = [IconSource {
            path: "textures/items/masked.png".into(),
            sprite: 1,
        }];
        let bytes = encode_icon_catalog_with_sources(
            [1; 32],
            &sprites,
            &[IconEntry {
                identifier: "test:masked".into(),
                metadata: 4,
                sprite: 0,
            }],
            &[],
            &[],
            &sources,
        )
        .unwrap();
        let catalog = RuntimeIconCatalog::decode(&bytes).unwrap();
        assert_eq!(
            catalog.lookup("test:masked", 4).unwrap().rgba8.as_ref(),
            &[12, 34, 56, 255]
        );
        let source = &catalog.sources().unwrap()[0];
        assert_eq!(source, &sources[0]);
        assert_eq!(
            catalog.sprites()[source.sprite as usize].rgba8.as_ref(),
            &[200, 210, 220, 96]
        );
        for path in [
            "../bad.png",
            "textures/items/../bad.png",
            "textures/items/UPPER.png",
        ] {
            assert!(
                encode_icon_catalog_with_sources(
                    [1; 32],
                    &sprites,
                    &[],
                    &[],
                    &[],
                    &[IconSource {
                        path: path.into(),
                        sprite: 0
                    }]
                )
                .is_err()
            );
        }
    }
}
