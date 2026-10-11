//! Immutable animation timelines, retained independently from image residency.

use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, sync::Arc};

use json_ui::{AsepriteFrame, parse_aseprite_frames};

use {
    super::{MAX_EXTRA, VANILLA_IN_PACKAGE, exact_case, vanilla_path},
    resource_pack::MAX_PACK_TEXTURE_BYTES,
};

pub(super) type Frames = Arc<[AsepriteFrame]>;

#[derive(Default)]
pub(super) struct FrameSidecars {
    inline: BTreeMap<String, Frames>,
    pack: Option<resource_pack::LayeredPackView>,
    vanilla: Option<PathBuf>,
    loaded: RefCell<BTreeMap<String, Option<Frames>>>,
}

impl FrameSidecars {
    pub(super) fn new(
        files: &[(String, Vec<u8>)],
        pack: Option<resource_pack::LayeredPackView>,
    ) -> Self {
        Self {
            inline: files
                .iter()
                .filter_map(|(path, bytes)| {
                    Some((path.strip_suffix(".json")?.to_owned(), parse(bytes)?))
                })
                .collect(),
            pack,
            ..Self::default()
        }
    }

    pub(super) fn set_vanilla(&mut self, vanilla: Option<PathBuf>) {
        if self.vanilla != vanilla {
            self.vanilla = vanilla;
            self.loaded.get_mut().clear();
        }
    }

    /// Starts from frames a worker already read from this stack's pack.
    pub(super) fn seed(&mut self, frames: &BTreeMap<String, Frames>) {
        self.loaded.get_mut().extend(
            frames
                .iter()
                .map(|(key, frames)| (key.clone(), Some(Arc::clone(frames)))),
        );
    }

    pub(super) fn get(&self, key: &str) -> Option<Frames> {
        if let Some(frames) = self.inline.get(key) {
            return Some(Arc::clone(frames));
        }
        if let Some(frames) = self.loaded.borrow().get(key) {
            return frames.clone();
        }
        let found = self
            .pack
            .as_ref()
            .and_then(|pack| pack.read_capped(&format!("{key}.json"), MAX_PACK_TEXTURE_BYTES))
            .and_then(|bytes| parse(&bytes))
            .or_else(|| self.local(key));
        let mut loaded = self.loaded.borrow_mut();
        if loaded.len() >= MAX_EXTRA {
            loaded.pop_first();
        }
        loaded.insert(key.to_owned(), found.clone());
        found
    }

    fn local(&self, key: &str) -> Option<Frames> {
        let relative = key.strip_prefix(VANILLA_IN_PACKAGE).unwrap_or(key);
        (key.starts_with("textures/") || relative != key).then_some(())?;
        let path = vanilla_path(self.vanilla.as_ref()?, &format!("{relative}.json"))?;
        exact_case(&path).then_some(())?;
        (std::fs::metadata(&path).ok()?.len() <= MAX_PACK_TEXTURE_BYTES).then_some(())?;
        parse(&std::fs::read(path).ok()?)
    }
}

fn parse(bytes: &[u8]) -> Option<Frames> {
    (bytes.len() as u64 <= MAX_PACK_TEXTURE_BYTES).then_some(())?;
    let text = resource_pack::normalize_jsonc(bytes)?;
    let frames = parse_aseprite_frames(&serde_json::from_slice(&text).ok()?)?;
    let total = frames.iter().try_fold(0_i64, |sum, frame| {
        (frame.x >= 0 && frame.y >= 0 && frame.duration_ms > 0).then_some(())?;
        sum.checked_add(frame.duration_ms)
    })?;
    (total > 0).then(|| frames.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_timeline_reads_share_storage() {
        let frames = FrameSidecars::new(
            &[(
                "textures/ui/noise.json".into(),
                br#"{"frames":[{"frame":{"x":8,"y":0},"duration":50}]}"#.to_vec(),
            )],
            None,
        );
        let first = frames.get("textures/ui/noise").unwrap();
        assert!(Arc::ptr_eq(
            &first,
            &frames.get("textures/ui/noise").unwrap()
        ));
    }

    #[test]
    fn invalid_timelines_remain_unavailable() {
        for bytes in [
            br#"{"frames":[]}"#.as_slice(),
            br#"{"frames":[{"frame":{"x":0,"y":0},"duration":0}]}"#,
            br#"{"frames":[{"frame":{"x":0,"y":0},"duration":-1}]}"#,
            br#"{"frames":[{"frame":{"x":0,"y":0},"duration":9223372036854775807},{"frame":{"x":0,"y":0},"duration":1}]}"#,
        ] {
            assert!(parse(bytes).is_none());
        }
    }
}
