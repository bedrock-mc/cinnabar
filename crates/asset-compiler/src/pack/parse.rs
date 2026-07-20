use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::Read,
    marker::PhantomData,
    path::{Component, Path},
    str,
};

use assets::AssetError;
use serde::{
    Deserialize,
    de::{DeserializeOwned, IgnoredAny, MapAccess, Visitor},
};

const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_TEXTURE_KEYS: usize = 8_192;
pub(super) const MAX_TEXTURE_VARIANTS: usize = 256;
const MAX_TEXTURE_PATH_BYTES: usize = 4 * 1024;

pub(super) struct BoundedUniqueMap<V, const MAX: usize> {
    pub(super) entries: BTreeMap<String, V>,
    pub(super) issue: Option<BoundedMapIssue>,
}

pub(super) enum BoundedMapIssue {
    Duplicate(Box<str>),
    TooMany { count: usize },
}

impl<'de, V, const MAX: usize> Deserialize<'de> for BoundedUniqueMap<V, MAX>
where
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(BoundedUniqueMapVisitor::<V, MAX>(PhantomData))
    }
}

struct BoundedUniqueMapVisitor<V, const MAX: usize>(PhantomData<V>);

impl<'de, V, const MAX: usize> Visitor<'de> for BoundedUniqueMapVisitor<V, MAX>
where
    V: Deserialize<'de>,
{
    type Value = BoundedUniqueMap<V, MAX>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object with unique bounded keys")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut entries = BTreeMap::new();
        let mut issue = None;
        let mut count = 0;
        while let Some(key) = map.next_key::<String>()? {
            count += 1;
            if issue.is_some() {
                map.next_value::<IgnoredAny>()?;
                continue;
            }
            if entries.contains_key(&key) {
                map.next_value::<IgnoredAny>()?;
                issue = Some(BoundedMapIssue::Duplicate(key.into_boxed_str()));
                continue;
            }
            if count > MAX {
                map.next_value::<IgnoredAny>()?;
                issue = Some(BoundedMapIssue::TooMany { count });
                continue;
            }
            entries.insert(key, map.next_value()?);
        }
        if let Some(BoundedMapIssue::TooMany { count: issue_count }) = &mut issue {
            *issue_count = count;
        }
        Ok(BoundedUniqueMap { entries, issue })
    }
}

pub(super) fn validate_texture_path(path: &str) -> Result<(), AssetError> {
    if path.len() > MAX_TEXTURE_PATH_BYTES {
        return Err(AssetError::TexturePathTooLong {
            path: path.into(),
            length: path.len(),
            max: MAX_TEXTURE_PATH_BYTES,
        });
    }

    let source_path = Path::new(path);
    let bytes = path.as_bytes();
    let has_windows_drive_prefix =
        bytes.get(1) == Some(&b':') && bytes.first().is_some_and(u8::is_ascii_alphabetic);
    let has_portable_root = path.starts_with(['/', '\\']);
    let has_portable_parent = path.split(['/', '\\']).any(|component| component == "..");
    let unsafe_component = source_path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if path.is_empty()
        || source_path.is_absolute()
        || has_windows_drive_prefix
        || has_portable_root
        || has_portable_parent
        || unsafe_component
    {
        return Err(AssetError::UnsafeTexturePath { path: path.into() });
    }
    Ok(())
}

pub(super) fn read_json<T: DeserializeOwned>(
    path: &Path,
    strip_comments: bool,
) -> Result<T, AssetError> {
    let file = File::open(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_JSON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(AssetError::JsonTooLarge {
            path: path.to_path_buf(),
            size: bytes.len(),
            max: MAX_JSON_BYTES,
        });
    }

    let text = str::from_utf8(&bytes).map_err(|source| AssetError::InvalidJsonUtf8 {
        path: path.to_path_buf(),
        source,
    })?;
    let text = if strip_comments {
        strip_leading_comment_lines(text)
    } else {
        text
    };
    serde_json::from_str(text).map_err(|source| AssetError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn strip_leading_comment_lines(input: &str) -> &str {
    let mut offset = 0;
    for line in input.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let trimmed = body.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            offset += line.len();
        } else {
            break;
        }
    }
    &input[offset..]
}
