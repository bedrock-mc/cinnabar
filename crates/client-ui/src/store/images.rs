//! Bounded in-memory cache of decoded offer images, keyed by source URL; the core has already fetched
//! and size-checked the files, this bounds what stays decoded.

use std::{collections::HashMap, fs::File, io::Cursor, io::Read, path::Path, sync::Arc};

const MAX_FILE_BYTES: u64 = 4 << 20;
const MAX_SIDE: u32 = 4096;
const MAX_ALLOC: u64 = 96 << 20;

/// A decoded RGBA8 image; the pixels are shared so the renderer can hold them past eviction.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

#[derive(Debug)]
pub enum ImageError {
    Io,
    TooLarge,
    Decode,
}

struct Entry {
    image: DecodedImage,
    used: u64,
}

/// LRU cache bounded by entry count and decoded bytes.
pub struct TextureCache {
    max_entries: usize,
    max_bytes: usize,
    bytes: usize,
    clock: u64,
    entries: HashMap<String, Entry>,
}

impl TextureCache {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
            bytes: 0,
            clock: 0,
            entries: HashMap::new(),
        }
    }

    /// The image for `key`, marking it recently used.
    pub fn get(&mut self, key: &str) -> Option<DecodedImage> {
        self.clock += 1;
        let clock = self.clock;
        self.entries.get_mut(key).map(|entry| {
            entry.used = clock;
            entry.image.clone()
        })
    }

    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Counts retained entries for the cache-boundary tests.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    /// Decode the file the core cached for `key`.
    pub fn load_file(&mut self, key: &str, path: &Path) -> Result<(), ImageError> {
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|_| ImageError::Io)?;
        self.load_bytes(key, &bytes)
    }

    /// Decode `bytes` (PNG, JPEG, GIF or BMP) into the cache, evicting the least recently used to fit.
    #[allow(
        clippy::field_reassign_with_default,
        reason = "image::Limits is non-exhaustive"
    )]
    pub fn load_bytes(&mut self, key: &str, bytes: &[u8]) -> Result<(), ImageError> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(ImageError::TooLarge);
        }
        let mut reader = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| ImageError::Decode)?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        limits.max_alloc = Some(MAX_ALLOC);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .map_err(|_| ImageError::Decode)?
            .into_rgba8();
        let (width, height) = decoded.dimensions();
        let size = decoded.as_raw().len();
        if size > self.max_bytes {
            return Err(ImageError::TooLarge);
        }
        self.remove(key);
        self.clock += 1;
        self.entries.insert(
            key.to_owned(),
            Entry {
                image: DecodedImage {
                    width,
                    height,
                    rgba: Arc::from(decoded.into_raw()),
                },
                used: self.clock,
            },
        );
        self.bytes += size;
        self.evict();
        Ok(())
    }

    fn remove(&mut self, key: &str) {
        if let Some(old) = self.entries.remove(key) {
            self.bytes -= old.image.rgba.len();
        }
    }

    fn evict(&mut self) {
        while self.entries.len() > self.max_entries || self.bytes > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            self.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(side: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::new(side, side)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode");
        out
    }

    #[test]
    fn decodes_and_reports_dimensions() {
        let mut cache = TextureCache::new(4, 1 << 20);
        cache.load_bytes("a", &png(4)).expect("decode");
        let image = cache.get("a").expect("cached");
        assert_eq!((image.width, image.height, image.rgba.len()), (4, 4, 64));
    }

    #[test]
    fn evicts_the_least_recently_used_by_count() {
        let mut cache = TextureCache::new(2, 1 << 20);
        cache.load_bytes("a", &png(2)).expect("a");
        cache.load_bytes("b", &png(2)).expect("b");
        assert!(cache.get("a").is_some());
        cache.load_bytes("c", &png(2)).expect("c");
        assert!(cache.contains("a") && cache.contains("c") && !cache.contains("b"));
    }

    #[test]
    fn evicts_by_decoded_bytes_and_refuses_an_image_that_alone_exceeds_them() {
        let mut cache = TextureCache::new(8, 2 * 4 * 4 * 4);
        cache.load_bytes("a", &png(4)).expect("a");
        cache.load_bytes("b", &png(4)).expect("b");
        cache.load_bytes("c", &png(4)).expect("c");
        assert_eq!(cache.len(), 2);
        assert!(matches!(
            cache.load_bytes("big", &png(16)),
            Err(ImageError::TooLarge)
        ));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn rejects_non_images_and_oversize_files() {
        let mut cache = TextureCache::new(2, 1 << 20);
        assert!(matches!(
            cache.load_bytes("x", b"not an image"),
            Err(ImageError::Decode)
        ));
        let huge = vec![0_u8; MAX_FILE_BYTES as usize + 1];
        assert!(matches!(
            cache.load_bytes("x", &huge),
            Err(ImageError::TooLarge)
        ));
        assert!(matches!(
            cache.load_file("x", Path::new("/definitely/not/here.png")),
            Err(ImageError::Io)
        ));
    }

    #[test]
    fn replacing_a_key_does_not_leak_bytes() {
        let mut cache = TextureCache::new(2, 1 << 20);
        cache.load_bytes("a", &png(4)).expect("first");
        cache.load_bytes("a", &png(4)).expect("second");
        assert_eq!((cache.len(), cache.bytes), (1, 64));
    }
}
