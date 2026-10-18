//! Schema-bounded catalog clip admission and immutable playback handles.
use super::*;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Descriptor {
    pub id: String,
    pub display_name: String,
    pub free: bool,
    pub clip: Asset,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    #[serde(deserialize_with = "clip_version")]
    pub version: u32,
    pub duration: f64,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub joint: String,
    pub keys: Vec<Key>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub time: f64,
    pub rotation: [f64; 4],
    pub offset: [f64; 3],
}

/// Accepts JSON's integer-valued numbers, including 1.0, without truncating fractional versions.
fn clip_version<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = f64::deserialize(deserializer)?;
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) || value.fract() != 0.0 {
        return Err(serde::de::Error::custom("invalid clip version"));
    }
    Ok(value as u32)
}

/// Logical parents are independent of the source skin's authored hierarchy.
#[derive(Deserialize)]
pub struct Joint {
    pub joint: String,
    pub parent: Option<String>,
    pub pivot: [f32; 3],
    #[serde(skip)]
    pub parent_index: Option<usize>,
}

/// Reads all clip budgets and the rig from the pinned API contract.
pub fn schema() -> &'static serde_json::Value {
    static SCHEMA: OnceLock<serde_json::Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../../../core/cinnabarapi/schema/v1.json"))
            .expect("pinned API schema")
    })
}

/// Returns the schema's logical rig in parent-before-child order.
pub fn rig() -> &'static [Joint] {
    static RIG: OnceLock<Vec<Joint>> = OnceLock::new();
    RIG.get_or_init(|| {
        let mut rig: Vec<Joint> =
            serde_json::from_value(schema()["$defs"]["EmoteClip"]["x-rig"].clone())
                .expect("clip rig");
        for index in 0..rig.len() {
            rig[index].parent_index = rig[index].parent.as_ref().map(|parent| {
                rig[..index]
                    .iter()
                    .position(|joint| joint.joint == *parent)
                    .expect("parent-before-child rig")
            });
        }
        rig
    })
}

/// Returns the maximum encoded clip size before JSON allocations are allowed.
pub fn max_bytes() -> usize {
    schema()["$defs"]["EmoteClip"]["x-max-bytes"]
        .as_u64()
        .expect("clip byte budget") as usize
}

impl Clip {
    /// Rejects malformed or over-budget channels, then normalizes validated rotations for playback.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > max_bytes() {
            return None;
        }
        let mut clip: Self = serde_json::from_slice(bytes).ok()?;
        let defs = &schema()["$defs"];
        let properties = &defs["EmoteClip"]["properties"];
        if u64::from(clip.version) != properties["version"]["const"].as_u64()?
            || !clip.duration.is_finite()
            || clip.duration <= 0.0
            || clip.duration > properties["duration"]["maximum"].as_f64()?
            || clip.tracks.is_empty()
            || clip.tracks.len() > properties["tracks"]["maxItems"].as_u64()? as usize
        {
            return None;
        }
        let mut count = 0;
        for (index, track) in clip.tracks.iter().enumerate() {
            count += track.keys.len();
            if !rig().iter().any(|joint| joint.joint == track.joint)
                || clip.tracks[..index]
                    .iter()
                    .any(|other| other.joint == track.joint)
                || track.keys.is_empty()
                || track.keys.len()
                    > defs["EmoteTrack"]["properties"]["keys"]["maxItems"].as_u64()? as usize
                || count > defs["EmoteClip"]["x-max-total-keyframes"].as_u64()? as usize
                || track.keys.first()?.time != 0.0
                || (track.keys.len() > 1 && track.keys.last()?.time != clip.duration)
            {
                return None;
            }
            for (index, key) in track.keys.iter().enumerate() {
                let norm = key.rotation.iter().map(|value| value.powi(2)).sum::<f64>();
                let offset_max =
                    defs["EmoteKey"]["properties"]["offset"]["items"]["maximum"].as_f64()?;
                let rotation_max =
                    defs["EmoteKey"]["properties"]["rotation"]["items"]["maximum"].as_f64()?;
                if !key.time.is_finite()
                    || key.time < 0.0
                    || key.time > clip.duration
                    || (index > 0 && key.time <= track.keys[index - 1].time)
                    || key
                        .rotation
                        .iter()
                        .any(|v| !v.is_finite() || v.abs() > rotation_max)
                    || (norm - 1.0).abs() > defs["EmoteClip"]["x-quaternion-tolerance"].as_f64()?
                    || key
                        .offset
                        .iter()
                        .any(|v| !v.is_finite() || v.abs() > offset_max)
                {
                    return None;
                }
            }
        }
        for track in &mut clip.tracks {
            for key in &mut track.keys {
                let length = key
                    .rotation
                    .iter()
                    .map(|value| value * value)
                    .sum::<f64>()
                    .sqrt();
                key.rotation = key.rotation.map(|value| value / length);
            }
        }
        Some(clip)
    }
}

/// An immutable admitted revision; a refresh cannot change active samples beneath the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Item(u16);
struct Admitted {
    descriptor: Descriptor,
    clip: Clip,
}
static ITEMS: RwLock<Vec<&'static Admitted>> = RwLock::new(Vec::new());
static ACTIVE: RwLock<Vec<Item>> = RwLock::new(Vec::new());
static REJECTED: AtomicU64 = AtomicU64::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Counts optional clips skipped at download, hash or parser admission boundaries.
pub fn reject() {
    REJECTED.fetch_add(1, Ordering::Relaxed);
}

/// Reports malformed or oversized clips without disconnecting a game session.
pub fn rejected() -> u64 {
    REJECTED.load(Ordering::Relaxed)
}

/// Checks metadata before downloading an optional clip.
pub fn supported(item: &Descriptor) -> bool {
    let valid_id = item.id.len() <= 128
        && item.id.split_once(':').is_some_and(|(namespace, name)| {
            let base = |v: u8| v.is_ascii_lowercase() || v.is_ascii_digit() || b"_.-".contains(&v);
            !namespace.is_empty()
                && namespace.bytes().all(base)
                && !name.is_empty()
                && name.bytes().all(base)
        });
    valid_id
        && !item.display_name.is_empty()
        && item.display_name.chars().count() <= 80
        && item.clip.bytes > 0
        && item.clip.bytes <= max_bytes()
        && super::valid_asset(&item.clip)
}

impl Item {
    /// Lists active clips owned by every account through the catalog's free flag.
    pub fn owned() -> Vec<Self> {
        Self::all()
            .into_iter()
            .filter(|item| item.descriptor().free)
            .collect()
    }
    /// Returns the current revision's stable catalog identifier.
    pub fn id(self) -> &'static str {
        &self.descriptor().id
    }
    /// Returns the catalog's display name for this revision.
    pub fn label(self) -> &'static str {
        &self.descriptor().display_name
    }
    /// Returns the authored clip duration in seconds.
    pub fn duration_seconds(self) -> f64 {
        self.clip().duration
    }
    /// Reports whether playback wraps at the authored duration.
    pub fn looping(self) -> bool {
        self.clip().looping
    }

    /// Hash-checks and parses optional content once, bounding retained immutable revisions.
    pub fn admit(descriptor: Descriptor, bytes: &[u8]) -> Option<Self> {
        let admitted = (|| {
            if !supported(&descriptor) || !descriptor.clip.verifies(bytes) {
                return None;
            }
            let clip = Clip::parse(bytes)?;
            let mut items = ITEMS.write().unwrap_or_else(|error| error.into_inner());
            if let Some(index) = items.iter().position(|item| item.descriptor == descriptor) {
                return Some(Self(index as u16));
            }
            let retained: usize = items.iter().map(|item| item.descriptor.clip.bytes).sum();
            let budget = schema()["$defs"]["EmoteClip"]["x-max-cache-bytes"].as_u64()? as usize;
            if items.len() >= MAX_ITEMS || retained + bytes.len() > budget {
                return None;
            }
            let index = items.len();
            items.push(Box::leak(Box::new(Admitted { descriptor, clip })));
            Some(Self(index as u16))
        })();
        if admitted.is_none() {
            reject();
        }
        admitted
    }

    /// Returns the admitted clip and its immutable metadata.
    pub fn clip(self) -> &'static Clip {
        &self.entry().clip
    }
    /// Returns the catalog metadata for the admitted revision.
    pub fn descriptor(self) -> &'static Descriptor {
        &self.entry().descriptor
    }
    /// Resolves the immutable storage backing a handle.
    fn entry(self) -> &'static Admitted {
        ITEMS.read().unwrap_or_else(|error| error.into_inner())[usize::from(self.0)]
    }
    /// Lists active, supported clips; free clips are owned by every account.
    pub fn all() -> Vec<Self> {
        ACTIVE
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    /// Resolves shared or saved identifiers only against the current catalog.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|item| item.descriptor().id == id)
    }
    /// Detects catalog refreshes so saved wheel IDs can be resolved again.
    pub fn generation() -> u64 {
        GENERATION.load(Ordering::Acquire)
    }
}

/// Publishes the complete supported catalog after all clip assets have been checked.
pub fn activate(items: &[Item]) {
    let active = items.to_vec();
    let mut current = ACTIVE.write().unwrap_or_else(|error| error.into_inner());
    if *current != active {
        *current = active;
        GENERATION.fetch_add(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
