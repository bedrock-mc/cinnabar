//! Decoded pack textures as raster pages, plus the sidecar metadata layout and
//! nine-slicing read. A texture resolves from the topmost layer holding it; its
//! sidecar follows the client's rule (a `base_size` is required, else the
//! image's own size with no slicing).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use json_ui::{TextureMeta, TextureSource};

use crate::raster::Page;
use crate::workspace::{TextureFile, Workspace};

/// Largest decoded texture edge admitted.
const MAX_EDGE: u32 = 4096;

#[derive(Clone)]
enum Entry {
    Ready {
        page: usize,
        meta: TextureMeta,
    },
    /// Known to a layer whose bytes the host has not supplied yet.
    Wanted {
        layer: usize,
        path: String,
    },
    Missing,
    Undecodable,
}

/// Decoded textures kept across renders until the files beneath them change.
#[derive(Default)]
pub struct TextureCache {
    entries: HashMap<String, Entry>,
    pages: Vec<Page>,
    built_for: (u64, u64),
}

impl TextureCache {
    /// Drop everything decoded when layers or texture bytes changed.
    pub fn sync(&mut self, workspace: &Workspace) {
        let key = (workspace.generation(), workspace.texture_generation());
        if key != self.built_for {
            self.entries.clear();
            self.pages.clear();
            self.built_for = key;
        }
    }

    pub fn page(&self, index: usize) -> Option<&Page> {
        self.pages.get(index)
    }
}

/// The texture view one render uses: lookups decode on first use.
pub struct Textures<'a> {
    workspace: RefCell<&'a mut Workspace>,
    cache: RefCell<&'a mut TextureCache>,
    /// `(layer, path)` files a render needed but the host has not supplied.
    pub wanted: RefCell<BTreeSet<(usize, String)>>,
    pub missing: RefCell<BTreeSet<String>>,
}

impl<'a> Textures<'a> {
    pub fn new(workspace: &'a mut Workspace, cache: &'a mut TextureCache) -> Self {
        cache.sync(workspace);
        Self {
            workspace: RefCell::new(workspace),
            cache: RefCell::new(cache),
            wanted: RefCell::default(),
            missing: RefCell::default(),
        }
    }

    /// The raster page index and pixel size `path` draws from.
    pub fn sprite(&self, path: &str) -> Option<(usize, [f64; 2])> {
        match self.entry(path) {
            Entry::Ready { page, .. } => {
                let cache = self.cache.borrow();
                let page_ref = &cache.pages[page];
                Some((
                    page,
                    [f64::from(page_ref.width), f64::from(page_ref.height)],
                ))
            }
            _ => None,
        }
    }

    pub fn with_page<R>(&self, index: usize, read: impl FnOnce(&Page) -> R) -> Option<R> {
        self.cache.borrow().pages.get(index).map(read)
    }

    fn entry(&self, path: &str) -> Entry {
        let key = texture_key(path).to_owned();
        if let Some(entry) = self.cache.borrow().entries.get(&key) {
            self.note(&key, entry);
            return entry.clone();
        }
        let entry = self.load(&key);
        self.note(&key, &entry);
        self.cache.borrow_mut().entries.insert(key, entry.clone());
        entry
    }

    fn note(&self, key: &str, entry: &Entry) {
        match entry {
            Entry::Wanted { layer, path } => {
                self.wanted.borrow_mut().insert((*layer, path.clone()));
            }
            Entry::Missing | Entry::Undecodable => {
                self.missing.borrow_mut().insert(key.to_owned());
            }
            Entry::Ready { .. } => {}
        }
    }

    fn load(&self, key: &str) -> Entry {
        if key.starts_with("http://") || key.starts_with("https://") {
            return Entry::Missing;
        }
        let mut workspace = self.workspace.borrow_mut();
        let (bytes, path) = match workspace.texture_file(key) {
            None => return Entry::Missing,
            Some(TextureFile::Wanted { layer, path }) => {
                return Entry::Wanted { layer, path };
            }
            Some(TextureFile::Loaded { bytes, path, .. }) => (bytes, path),
        };
        let Some(page) = decode(&bytes, &path) else {
            return Entry::Undecodable;
        };
        let size = [f64::from(page.width), f64::from(page.height)];
        let meta = workspace
            .sidecar(key)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .and_then(|value| json_ui::parse_texture_meta(&value))
            .map(|meta| TextureMeta {
                pixels: size,
                ..meta
            })
            .unwrap_or(TextureMeta::plain(size));
        let mut cache = self.cache.borrow_mut();
        cache.pages.push(page);
        Entry::Ready {
            page: cache.pages.len() - 1,
            meta,
        }
    }
}

impl TextureSource for Textures<'_> {
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        match self.entry(path) {
            Entry::Ready { meta, .. } => Some(meta),
            _ => None,
        }
    }
}

/// Ui json sometimes spells a texture with its extension; lookups key it without.
pub fn texture_key(path: &str) -> &str {
    let path = path.trim_start_matches('/');
    for extension in [".png", ".jpg", ".jpeg", ".tga"] {
        if let Some(stem) = path.strip_suffix(extension) {
            return stem;
        }
    }
    path
}

fn decode(bytes: &[u8], path: &str) -> Option<Page> {
    let image = decode_image(bytes, path).ok()?;
    if image.width() > MAX_EDGE || image.height() > MAX_EDGE {
        return None;
    }
    let rgba = image.into_rgba8();
    Some(Page {
        width: rgba.width(),
        height: rgba.height(),
        rgba: Arc::from(rgba.into_raw()),
    })
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn review_pending_texture_is_requested_again_on_a_cache_hit() {
        let mut workspace = Workspace::default();
        let layer = workspace.add_layer("pack");
        workspace.add_files(
            layer,
            vec![("manifest.json".into(), b"{}".to_vec())],
            vec!["textures/ui/pending.png".into()],
        );
        let mut cache = TextureCache::default();
        for render in 0..2 {
            let textures = Textures::new(&mut workspace, &mut cache);
            assert!(textures.sprite("textures/ui/pending").is_none());
            assert!(
                textures
                    .wanted
                    .borrow()
                    .contains(&(layer, "textures/ui/pending.png".into())),
                "render {render} must retain the pending-byte request"
            );
        }
    }
}

/// Decodes texture pixels using the editor's image format selection.
fn decode_image(bytes: &[u8], path: &str) -> image::ImageResult<image::DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
    if path.to_ascii_lowercase().ends_with(".tga") {
        reader.set_format(image::ImageFormat::Tga);
    } else {
        reader = reader.with_guessed_format()?;
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(u64::from(MAX_EDGE) * u64::from(MAX_EDGE) * 8);
    reader.limits(limits);
    reader.decode()
}

#[cfg(test)]
mod review_limits_tests {
    use super::*;
    #[test]
    fn review_oversized_texture_is_rejected_before_reading_pixel_data() {
        let mut header = [0_u8; 18];
        header[2] = 2;
        header[12..14].copy_from_slice(&((MAX_EDGE + 1) as u16).to_le_bytes());
        header[14..16].copy_from_slice(&1_u16.to_le_bytes());
        header[16] = 32;
        assert!(matches!(
            decode_image(&header, "oversized.tga"),
            Err(image::ImageError::Limits(_))
        ));
    }
}
