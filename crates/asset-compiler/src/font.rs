use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
};

use assets::{
    FONT_CARRIER_SCHEMA, FontCatalogError, FontTexturePage, GlyphMetrics, MAX_FONT_GLYPHS,
    MAX_FONT_PAGE_SIDE, MAX_FONT_PAGES, MAX_FONT_PATH_BYTES, MAX_FONT_SOURCE_BYTES,
    encode_font_catalog,
};
use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

mod outline;

pub use outline::{OutlineFontConfig, compile_outline_font};

const DESCRIPTOR_PATH: &str = "font/catalog.json";
const PINNED_SOURCE_MANIFEST_SHA256: [u8; 32] =
    decode_sha256(b"c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontCompileReport {
    pub schema: u32,
    pub glyphs: usize,
    pub pages: usize,
    pub source_bytes: u64,
    pub decoded_bytes: u64,
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledFontCarrier {
    pub bytes: Box<[u8]>,
    pub report: FontCompileReport,
}

#[derive(Debug, Error)]
pub enum FontCompileError {
    #[error("failed to inspect font source {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("font source {path} is empty or exceeds the aggregate source-byte bound")]
    SourceTooLarge { path: PathBuf },
    #[error("font descriptor JSON is invalid: {source}")]
    DescriptorJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("font descriptor source manifest is not the reviewed pin")]
    SourceManifestMismatch,
    #[error("invalid font descriptor: {detail}")]
    InvalidDescriptor { detail: Box<str> },
    #[error("font metric {field} for U+{codepoint:04X} is non-finite")]
    NonFiniteMetric { codepoint: u32, field: &'static str },
    #[error("font metric {field} for U+{codepoint:04X} is outside its carrier representation")]
    MetricOutOfRange { codepoint: u32, field: &'static str },
    #[error("font page {path} is {width}x{height}, exceeding side limit {max}")]
    PageTooLarge {
        path: PathBuf,
        width: u32,
        height: u32,
        max: u32,
    },
    #[error("failed to decode font page {path}: {source}")]
    PageDecode {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("duplicate semantic glyph U+{codepoint:04X}")]
    DuplicateGlyph { codepoint: u32 },
    #[error("outline font {path} is invalid: {detail}")]
    OutlineFont { path: PathBuf, detail: Box<str> },
    #[error("outline font atlas cannot fit the reviewed glyph set within {side}x{side}")]
    OutlineAtlasFull { side: u32 },
    #[error(transparent)]
    Carrier(#[from] FontCatalogError),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    schema: u32,
    source_manifest_sha256: Box<str>,
    pages: Vec<PageDescriptor>,
    glyphs: Vec<GlyphDescriptor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageDescriptor {
    source: Box<str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlyphDescriptor {
    codepoint: u32,
    page: Box<str>,
    uv: [u32; 4],
    bearing: [MetricValue; 2],
    advance: MetricValue,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MetricValue {
    Number(f64),
    Text(Box<str>),
}

impl MetricValue {
    fn value(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(value) => value.parse().ok(),
        }
    }
}

/// Compiles the reviewed bitmap-font descriptor and its referenced PNG pages.
/// Compressed source payloads remain local; only identity, metrics, and raw
/// unpremultiplied RGBA8 enter the deterministic carrier.
pub fn compile_fonts(root: &Path) -> Result<CompiledFontCarrier, FontCompileError> {
    let font_root = root.join("font");
    require_real_directory(&font_root)?;
    let descriptor_source = resolve_real_source(root, DESCRIPTOR_PATH)?;
    let descriptor_path = descriptor_source.candidate.clone();
    let descriptor_bytes = read_source(&descriptor_source, MAX_FONT_SOURCE_BYTES)?;
    let descriptor = serde_json::from_slice::<Descriptor>(&descriptor_bytes)
        .map_err(|source| FontCompileError::DescriptorJson { source })?;
    if descriptor.schema != FONT_CARRIER_SCHEMA
        || decode_runtime_sha256(&descriptor.source_manifest_sha256)
            != Some(PINNED_SOURCE_MANIFEST_SHA256)
    {
        return Err(FontCompileError::SourceManifestMismatch);
    }
    if descriptor.pages.is_empty() || descriptor.pages.len() > MAX_FONT_PAGES {
        return Err(invalid("font page count is outside its bound"));
    }
    if descriptor.glyphs.is_empty() || descriptor.glyphs.len() > MAX_FONT_GLYPHS {
        return Err(invalid("font glyph count is outside its bound"));
    }

    let mut declared_pages = BTreeSet::new();
    for page in &descriptor.pages {
        validate_source_path(&page.source)?;
        if !declared_pages.insert(page.source.clone()) {
            return Err(invalid("duplicate font page source path"));
        }
    }

    let mut total_source_bytes =
        u64::try_from(descriptor_bytes.len()).map_err(|_| FontCompileError::SourceTooLarge {
            path: descriptor_path.clone(),
        })?;
    let mut total_decoded_bytes = 0u64;
    let mut pages = Vec::with_capacity(descriptor.pages.len());
    for page in descriptor.pages {
        let source = resolve_real_source(root, &page.source)?;
        let path = source.candidate.clone();
        let remaining = MAX_FONT_SOURCE_BYTES
            .checked_sub(total_source_bytes)
            .ok_or_else(|| FontCompileError::SourceTooLarge { path: path.clone() })?;
        let bytes = read_source(&source, remaining)?;
        total_source_bytes = total_source_bytes
            .checked_add(
                u64::try_from(bytes.len())
                    .map_err(|_| FontCompileError::SourceTooLarge { path: path.clone() })?,
            )
            .ok_or_else(|| FontCompileError::SourceTooLarge { path: path.clone() })?;
        let dimensions = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png)
            .into_dimensions()
            .map_err(|source| FontCompileError::PageDecode {
                path: path.clone(),
                source: Box::new(source),
            })?;
        if dimensions.0 == 0
            || dimensions.1 == 0
            || dimensions.0 > MAX_FONT_PAGE_SIDE
            || dimensions.1 > MAX_FONT_PAGE_SIDE
        {
            return Err(FontCompileError::PageTooLarge {
                path,
                width: dimensions.0,
                height: dimensions.1,
                max: MAX_FONT_PAGE_SIDE,
            });
        }
        let decoded_length = u64::from(dimensions.0)
            .checked_mul(u64::from(dimensions.1))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| FontCompileError::PageTooLarge {
                path: path.clone(),
                width: dimensions.0,
                height: dimensions.1,
                max: MAX_FONT_PAGE_SIDE,
            })?;
        total_decoded_bytes = total_decoded_bytes
            .checked_add(decoded_length)
            .ok_or_else(|| FontCompileError::SourceTooLarge { path: path.clone() })?;
        if total_decoded_bytes > MAX_FONT_SOURCE_BYTES {
            return Err(FontCompileError::SourceTooLarge { path });
        }
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_FONT_PAGE_SIDE);
        limits.max_image_height = Some(MAX_FONT_PAGE_SIDE);
        limits.max_alloc = Some(decoded_length);
        reader.limits(limits);
        let rgba8 = reader
            .decode()
            .map_err(|source| FontCompileError::PageDecode {
                path: path.clone(),
                source: Box::new(source),
            })?
            .into_rgba8()
            .into_raw()
            .into_boxed_slice();
        if u64::try_from(rgba8.len()).ok() != Some(decoded_length) {
            return Err(invalid("font page decoded to a noncanonical RGBA8 length"));
        }
        pages.push(FontTexturePage {
            source_path: page.source,
            source_bytes: u32::try_from(bytes.len())
                .map_err(|_| FontCompileError::SourceTooLarge { path: path.clone() })?,
            source_sha256: Sha256::digest(&bytes).into(),
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width: dimensions.0,
            height: dimensions.1,
            rgba8,
        });
    }
    pages.sort_by(|left, right| {
        (&left.source_path, left.source_sha256).cmp(&(&right.source_path, right.source_sha256))
    });
    let page_indices = pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.source_path.as_ref(), index))
        .collect::<BTreeMap<_, _>>();

    let mut glyphs = descriptor
        .glyphs
        .into_iter()
        .map(|glyph| compile_glyph(glyph, &page_indices, &pages))
        .collect::<Result<Vec<_>, _>>()?;
    glyphs.sort_by_key(|glyph| {
        (
            glyph.codepoint as u32,
            &pages[usize::from(glyph.page)].source_path,
            pages[usize::from(glyph.page)].source_sha256,
        )
    });
    for pair in glyphs.windows(2) {
        if pair[0].codepoint == pair[1].codepoint {
            return Err(FontCompileError::DuplicateGlyph {
                codepoint: pair[0].codepoint as u32,
            });
        }
    }

    let bytes = encode_font_catalog(PINNED_SOURCE_MANIFEST_SHA256, &glyphs, &pages)?;
    let carrier_sha256 = bytes
        .get(bytes.len().saturating_sub(32)..)
        .and_then(|digest| digest.try_into().ok())
        .ok_or_else(|| invalid("encoded font carrier lacks its SHA-256"))?;
    Ok(CompiledFontCarrier {
        report: FontCompileReport {
            schema: FONT_CARRIER_SCHEMA,
            glyphs: glyphs.len(),
            pages: pages.len(),
            source_bytes: total_source_bytes,
            decoded_bytes: total_decoded_bytes,
            source_manifest_sha256: PINNED_SOURCE_MANIFEST_SHA256,
            carrier_sha256,
        },
        bytes,
    })
}

fn compile_glyph(
    glyph: GlyphDescriptor,
    page_indices: &BTreeMap<&str, usize>,
    pages: &[FontTexturePage],
) -> Result<GlyphMetrics, FontCompileError> {
    let codepoint = char::from_u32(glyph.codepoint)
        .ok_or_else(|| invalid("font glyph codepoint is not a Unicode scalar"))?;
    let page_index = page_indices
        .get(glyph.page.as_ref())
        .copied()
        .ok_or_else(|| invalid("font glyph references an undeclared page"))?;
    let page = &pages[page_index];
    let uv: [u16; 4] = glyph
        .uv
        .map(u16::try_from)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| FontCompileError::MetricOutOfRange {
            codepoint: glyph.codepoint,
            field: "uv",
        })?
        .try_into()
        .map_err(|_| invalid("font UV field has the wrong length"))?;
    if uv[0] >= uv[2]
        || uv[1] >= uv[3]
        || u32::from(uv[2]) > page.width
        || u32::from(uv[3]) > page.height
    {
        return Err(FontCompileError::MetricOutOfRange {
            codepoint: glyph.codepoint,
            field: "uv",
        });
    }
    let bearing = [
        integral_i16(&glyph.bearing[0], glyph.codepoint, "bearing_x")?,
        integral_i16(&glyph.bearing[1], glyph.codepoint, "bearing_y")?,
    ];
    let advance = finite_metric(&glyph.advance, glyph.codepoint, "advance")?;
    let advance_scaled = advance * 64.0;
    if !advance_scaled.is_finite()
        || advance_scaled.fract() != 0.0
        || advance_scaled < f64::from(i16::MIN)
        || advance_scaled > f64::from(i16::MAX)
    {
        return Err(FontCompileError::MetricOutOfRange {
            codepoint: glyph.codepoint,
            field: "advance",
        });
    }
    Ok(GlyphMetrics {
        codepoint,
        page: u16::try_from(page_index).map_err(|_| FontCompileError::MetricOutOfRange {
            codepoint: glyph.codepoint,
            field: "page",
        })?,
        uv,
        bearing,
        advance_64: advance_scaled as i16,
    })
}

fn integral_i16(
    metric: &MetricValue,
    codepoint: u32,
    field: &'static str,
) -> Result<i16, FontCompileError> {
    let value = finite_metric(metric, codepoint, field)?;
    if value.fract() != 0.0 || value < f64::from(i16::MIN) || value > f64::from(i16::MAX) {
        return Err(FontCompileError::MetricOutOfRange { codepoint, field });
    }
    Ok(value as i16)
}

fn finite_metric(
    metric: &MetricValue,
    codepoint: u32,
    field: &'static str,
) -> Result<f64, FontCompileError> {
    let value = metric
        .value()
        .ok_or(FontCompileError::NonFiniteMetric { codepoint, field })?;
    if !value.is_finite() {
        return Err(FontCompileError::NonFiniteMetric { codepoint, field });
    }
    Ok(value)
}

fn require_real_directory(path: &Path) -> Result<(), FontCompileError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| FontCompileError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return Err(invalid("font root must be a real directory"));
    }
    Ok(())
}

struct ResolvedSource {
    candidate: PathBuf,
    root_handle: File,
    #[cfg(unix)]
    file_name: Box<str>,
}

fn resolve_real_source(root: &Path, relative: &str) -> Result<ResolvedSource, FontCompileError> {
    validate_source_path_for_extension(relative)?;
    let font_root = root.join("font");
    let root_handle = open_font_root(&font_root).map_err(|source| FontCompileError::Io {
        path: font_root,
        source,
    })?;
    #[cfg(unix)]
    let file_name = relative
        .strip_prefix("font/")
        .ok_or_else(|| invalid("font source path is outside the flat font root"))?;
    let candidate = root.join(relative);
    let metadata = fs::symlink_metadata(&candidate).map_err(|source| FontCompileError::Io {
        path: candidate.clone(),
        source,
    })?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(invalid(
            "font source is a symlink, reparse point, or not a regular file",
        ));
    }
    Ok(ResolvedSource {
        candidate,
        root_handle,
        #[cfg(unix)]
        file_name: file_name.into(),
    })
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(unix)]
fn open_font_root(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(unix_open_flags::NOFOLLOW | unix_open_flags::DIRECTORY)
        .open(path)?;
    if !file.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "font root handle is not a directory",
        ));
    }
    Ok(file)
}

#[cfg(windows)]
fn open_font_root(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "font root handle is a reparse point or not a directory",
        ));
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn open_font_root(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure font source opening is unsupported on this platform",
    ))
}

#[cfg(unix)]
fn open_source_handle(source: &ResolvedSource) -> io::Result<File> {
    use std::{
        ffi::CString,
        os::unix::io::{AsRawFd, FromRawFd},
    };

    let file_name = CString::new(source.file_name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "font file name contains NUL"))?;
    let flags = unix_open_flags::NOFOLLOW | unix_open_flags::CLOEXEC;
    // SAFETY: `root_handle` owns a live directory descriptor, `file_name` is
    // one NUL-terminated name, and no create flag is used.
    let descriptor = unsafe { openat(source.root_handle.as_raw_fd(), file_name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful `openat` returns one newly owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "font source handle is not a regular file",
        ));
    }
    Ok(file)
}

#[cfg(unix)]
unsafe extern "C" {
    fn openat(
        directory: std::os::raw::c_int,
        path: *const std::os::raw::c_char,
        flags: std::os::raw::c_int,
    ) -> std::os::raw::c_int;
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
mod unix_open_flags {
    pub const DIRECTORY: i32 = 0x1_0000;
    pub const NOFOLLOW: i32 = 0x2_0000;
    pub const CLOEXEC: i32 = 0x8_0000;
}

#[cfg(all(unix, any(target_os = "macos", target_os = "ios")))]
mod unix_open_flags {
    pub const DIRECTORY: i32 = 0x10_0000;
    pub const NOFOLLOW: i32 = 0x100;
    pub const CLOEXEC: i32 = 0x100_0000;
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))
))]
compile_error!("secure font source opening requires reviewed openat flags for this Unix target");

#[cfg(windows)]
fn open_source_handle(source: &ResolvedSource) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&source.candidate)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "font source handle is a reparse point or not a file",
        ));
    }
    let root_path = windows_final_path(&source.root_handle)?;
    let file_path = windows_final_path(&file)?;
    if file_path == root_path || !file_path.starts_with(&root_path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "opened font source handle resolves outside the bound font root handle",
        ));
    }
    Ok(file)
}

#[cfg(windows)]
fn windows_final_path(file: &File) -> io::Result<PathBuf> {
    use std::{
        ffi::OsString,
        os::windows::{ffi::OsStringExt, io::AsRawHandle},
    };

    const MAX_FINAL_PATH_UNITS: usize = 32_768;
    let mut buffer = vec![0_u16; 512];
    loop {
        let capacity = u32::try_from(buffer.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Windows final path is too long")
        })?;
        // SAFETY: the file owns a live handle and the buffer exposes exactly
        // `capacity` writable UTF-16 units for the duration of this call.
        let written = unsafe {
            GetFinalPathNameByHandleW(file.as_raw_handle(), buffer.as_mut_ptr(), capacity, 0)
        };
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = usize::try_from(written).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Windows final path is too long")
        })?;
        if length < buffer.len() {
            buffer.truncate(length);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        if length >= MAX_FINAL_PATH_UNITS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows final path exceeds its bound",
            ));
        }
        buffer.resize(length + 1, 0);
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFinalPathNameByHandleW(
        file: std::os::windows::io::RawHandle,
        path: *mut u16,
        path_units: u32,
        flags: u32,
    ) -> u32;
}

#[cfg(not(any(unix, windows)))]
fn open_source_handle(_source: &ResolvedSource) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure font source opening is unsupported on this platform",
    ))
}

fn read_source(source: &ResolvedSource, limit: u64) -> Result<Vec<u8>, FontCompileError> {
    let file = open_source_handle(source).map_err(|error| FontCompileError::Io {
        path: source.candidate.clone(),
        source: error,
    })?;
    let length = file
        .metadata()
        .map_err(|error| FontCompileError::Io {
            path: source.candidate.clone(),
            source: error,
        })?
        .len();
    if length == 0 || length > limit {
        return Err(FontCompileError::SourceTooLarge {
            path: source.candidate.clone(),
        });
    }
    let capacity = usize::try_from(length).map_err(|_| FontCompileError::SourceTooLarge {
        path: source.candidate.clone(),
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| FontCompileError::Io {
            path: source.candidate.clone(),
            source: error,
        })?;
    if u64::try_from(bytes.len())
        .ok()
        .is_none_or(|length| length > limit)
    {
        return Err(FontCompileError::SourceTooLarge {
            path: source.candidate.clone(),
        });
    }
    Ok(bytes)
}

fn validate_source_path(path: &str) -> Result<(), FontCompileError> {
    validate_source_path_for_extension(path)?;
    if !path.ends_with(".png") {
        return Err(invalid("font page path is unsafe or noncanonical"));
    }
    Ok(())
}

fn validate_source_path_for_extension(path: &str) -> Result<(), FontCompileError> {
    let mut components = path.split('/');
    let root = components.next();
    let file_name = components.next();
    if path.is_empty()
        || path.len() > MAX_FONT_PATH_BYTES
        || path.contains('\\')
        || root != Some("font")
        || file_name.is_none_or(|name| name.is_empty() || name == "." || name == "..")
        || components.next().is_some()
    {
        return Err(invalid("font page path is unsafe or noncanonical"));
    }
    Ok(())
}

fn decode_runtime_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut decoded = [0; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] =
            (decode_hex_nibble_runtime(pair[0])? << 4) | decode_hex_nibble_runtime(pair[1])?;
    }
    Some(decoded)
}

fn decode_hex_nibble_runtime(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

const fn decode_sha256(value: &[u8; 64]) -> [u8; 32] {
    let mut decoded = [0; 32];
    let mut index = 0;
    while index < decoded.len() {
        decoded[index] =
            (decode_hex_nibble(value[index * 2]) << 4) | decode_hex_nibble(value[index * 2 + 1]);
        index += 1;
    }
    decoded
}

const fn decode_hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("invalid pinned SHA-256"),
    }
}

fn invalid(detail: impl Into<Box<str>>) -> FontCompileError {
    FontCompileError::InvalidDescriptor {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod source_race_tests {
    use std::fs;

    use super::{FontCompileError, resolve_real_source};

    #[test]
    fn intermediate_retarget_paths_are_rejected_before_resolution() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("font")).unwrap();

        for path in ["font/outside/page.png", "font/inside/page.png"] {
            assert!(matches!(
                resolve_real_source(directory.path(), path),
                Err(FontCompileError::InvalidDescriptor { .. })
            ));
        }
    }
}
