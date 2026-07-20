//! Cover art derivative generation: square lossless WebP thumbnails and previews.
//!
//! Derivative identity is the SHA-256 of the original cover bytes, not the
//! song hash. Replacing/extracting cover art therefore produces new filenames
//! and cannot serve stale imagery. All output paths are stored relative to
//! the library root using forward slashes for portability.

use crate::library_root::{LibraryRoot, ARTWORK_DIRECTORY};
use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::{ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Cursor, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_INPUT_BYTES: usize = 20 * 1024 * 1024;
const MAX_INPUT_DIMENSION: u32 = 8_000;
const MAX_INPUT_PIXELS: u64 = 40_000_000;
const MAX_DECODE_ALLOC: u64 = 192 * 1024 * 1024;
pub const THUMB_SIZE: u32 = 80;
pub const PREVIEW_SIZE: u32 = 256;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtworkSize {
    Thumb,
    Preview,
}

impl ArtworkSize {
    pub(crate) fn expected_dimension(self) -> u32 {
        match self {
            Self::Thumb => THUMB_SIZE,
            Self::Preview => PREVIEW_SIZE,
        }
    }

    fn filename_prefix(self) -> &'static str {
        match self {
            Self::Thumb => "thumb_",
            Self::Preview => "preview_",
        }
    }

    fn filename_suffix(self) -> &'static str {
        match self {
            Self::Thumb => "_80.webp",
            Self::Preview => "_256.webp",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArtworkDerivatives {
    pub thumb_path: String,
    pub preview_path: String,
}

pub fn cover_sha256(original: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(original);
    crate::hash::hex_lower(hasher.finalize())
}

pub(crate) fn derivative_relative_path(size: ArtworkSize, digest: &str) -> String {
    format!(
        "artwork/{}{}{}",
        size.filename_prefix(),
        digest,
        size.filename_suffix()
    )
}

/// Files are written atomically to the library's `artwork/` directory using
/// digest-based deterministic filenames.
pub fn generate_artwork_derivatives(
    library: &LibraryRoot,
    original: &[u8],
) -> Result<ArtworkDerivatives> {
    if original.len() > MAX_INPUT_BYTES {
        anyhow::bail!("cover art input exceeds maximum size");
    }

    let digest = cover_sha256(original);
    let thumb_rel = derivative_relative_path(ArtworkSize::Thumb, &digest);
    let preview_rel = derivative_relative_path(ArtworkSize::Preview, &digest);

    let image = decode_with_limits(original)?;
    let rgba = image.to_rgba8();

    write_derivative(library, &thumb_rel, &rgba, ArtworkSize::Thumb)?;
    write_derivative(library, &preview_rel, &rgba, ArtworkSize::Preview)?;

    Ok(ArtworkDerivatives {
        thumb_path: thumb_rel,
        preview_path: preview_rel,
    })
}

/// Decode an image from bytes with pre-decode dimension and allocation limits.
fn decode_with_limits(original: &[u8]) -> Result<image::DynamicImage> {
    let reader = ImageReader::new(Cursor::new(original))
        .with_guessed_format()
        .context("failed to guess image format")?;

    // Probe dimensions before full decode to reject oversized images early.
    // into_dimensions consumes the reader; we re-create it for the full decode.
    let (width, height) = reader
        .into_dimensions()
        .context("failed to read image dimensions")?;
    if width > MAX_INPUT_DIMENSION || height > MAX_INPUT_DIMENSION {
        anyhow::bail!("cover art dimensions exceed maximum");
    }
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if pixels > MAX_INPUT_PIXELS {
        anyhow::bail!("cover art pixel count exceeds maximum");
    }

    let mut limited_reader = ImageReader::new(Cursor::new(original))
        .with_guessed_format()
        .context("failed to guess image format")?;
    let mut limits = Limits::no_limits();
    limits.max_image_width = Some(MAX_INPUT_DIMENSION);
    limits.max_image_height = Some(MAX_INPUT_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limited_reader.limits(limits);

    let image = limited_reader
        .decode()
        .context("failed to decode cover art")?;

    let (dw, dh) = (image.width(), image.height());
    if dw > MAX_INPUT_DIMENSION || dh > MAX_INPUT_DIMENSION {
        anyhow::bail!("decoded image dimensions exceed maximum");
    }
    let dp = u64::from(dw).saturating_mul(u64::from(dh));
    if dp > MAX_INPUT_PIXELS {
        anyhow::bail!("decoded image pixel count exceeds maximum");
    }

    Ok(image)
}

/// Uses `DynamicImage::resize_to_fill` so non-square cover art is scaled to
/// cover the target square and center-cropped, producing exactly square
/// derivatives without stretching the source aspect ratio.
fn encode_webp(rgba: &image::RgbaImage, size: u32) -> Result<Vec<u8>> {
    let dynamic = image::DynamicImage::ImageRgba8(rgba.clone());
    let resized = dynamic.resize_to_fill(size, size, FilterType::Lanczos3);
    let resized_rgba = resized.to_rgba8();
    let mut buf = Vec::new();
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
    encoder
        .encode(&resized_rgba, size, size, image::ExtendedColorType::Rgba8)
        .context("failed to encode WebP")?;
    Ok(buf)
}

/// If the final file already exists and is a valid WebP of the exact expected
/// dimensions, treat it as success.
fn write_derivative(
    library: &LibraryRoot,
    relative_path: &str,
    rgba: &image::RgbaImage,
    size: ArtworkSize,
) -> Result<()> {
    let (final_abs, path_size) = resolve_artwork_path(library, relative_path)?;
    if path_size != size {
        anyhow::bail!("artwork derivative filename does not match its requested size");
    }
    let expected_size = size.expected_dimension();

    let webp_bytes = encode_webp(rgba, expected_size)?;
    write_derivative_bytes(&final_abs, &webp_bytes, expected_size)
}

/// The final entry is checked with `symlink_metadata` so a symlink is removed
/// as a link, never followed as a destination outside `artwork/`.
fn write_derivative_bytes(final_abs: &Path, webp_bytes: &[u8], expected_size: u32) -> Result<()> {
    if fs::symlink_metadata(final_abs).is_ok() {
        if validate_derivative_file(final_abs, expected_size) {
            return Ok(());
        }
        // Invalid existing final (including a symlink) — remove the directory
        // entry, not a target outside artwork, then retry generation.
        fs::remove_file(final_abs).with_context(|| {
            format!(
                "failed to remove invalid derivative at {}",
                final_abs.display()
            )
        })?;
    }

    let temp_path = unique_temp_path(final_abs);

    let mut guard = TempFileGuard::new(&temp_path);

    {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| format!("failed to create temp file at {}", temp_path.display()))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(webp_bytes)
            .context("failed to write WebP bytes")?;
        writer.flush().context("failed to flush temp file")?;
        let file = writer
            .into_inner()
            .map_err(|e| anyhow::anyhow!("failed to recover temp file writer: {e}"))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp file at {}", temp_path.display()))?;
    }

    match fs::rename(&temp_path, final_abs) {
        Ok(()) => {
            guard.disarm();
            Ok(())
        }
        Err(_) => {
            // Race: another process may have created the final file. A valid
            // final wins and the temp is removed by the guard.
            if fs::symlink_metadata(final_abs).is_ok()
                && validate_derivative_file(final_abs, expected_size)
            {
                guard.disarm();
                let _ = fs::remove_file(&temp_path);
                Ok(())
            } else {
                Err(anyhow::anyhow!("failed to write artwork derivative"))
            }
        }
    }
}

/// Both source and destination paths are resolved through the strict
/// content-addressed parser; the destination write is atomic and never
/// follows an existing symlink.
pub(crate) fn copy_artwork_derivative(
    source_library: &LibraryRoot,
    destination_library: &LibraryRoot,
    relative_path: &str,
    expected_size: u32,
) -> Result<()> {
    let (source, source_size) = resolve_existing_artwork_path(source_library, relative_path)?;
    if source_size.expected_dimension() != expected_size
        || !validate_derivative_file(&source, expected_size)
    {
        anyhow::bail!("artwork derivative source is missing or invalid");
    }
    let bytes = fs::read(&source)
        .with_context(|| format!("failed to read artwork derivative at {}", source.display()))?;
    if !validate_derivative_bytes(&bytes, expected_size) {
        anyhow::bail!("artwork derivative source changed while being copied");
    }

    let (destination, destination_size) = resolve_artwork_path(destination_library, relative_path)?;
    if destination_size != source_size {
        anyhow::bail!("artwork derivative destination size does not match source");
    }
    write_derivative_bytes(&destination, &bytes, expected_size)
}

/// Ensures every error/unwind path cleans up its own temp file.
struct TempFileGuard<'a> {
    path: &'a Path,
    armed: bool,
}

impl<'a> TempFileGuard<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, armed: true }
    }

    /// Disarm the guard after the temp file has been consumed (renamed to the
    /// final path or removed after a race loss).
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(self.path);
        }
    }
}

/// A filename suffix alone is not authoritative: image decoders can identify
/// JPEG or PNG bytes in a `.webp` file.
pub(crate) fn validate_derivative_file(path: &Path, expected_size: u32) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return false;
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let Ok(reader) = ImageReader::new(std::io::BufReader::new(file)).with_guessed_format() else {
        return false;
    };
    validate_derivative_reader(reader, expected_size)
}

fn validate_derivative_bytes(bytes: &[u8], expected_size: u32) -> bool {
    let Ok(reader) = ImageReader::new(Cursor::new(bytes)).with_guessed_format() else {
        return false;
    };
    validate_derivative_reader(reader, expected_size)
}

fn validate_derivative_reader<R: std::io::BufRead + std::io::Seek>(
    reader: ImageReader<R>,
    expected_size: u32,
) -> bool {
    if reader.format() != Some(image::ImageFormat::WebP) {
        return false;
    }
    let Ok((w, h)) = reader.into_dimensions() else {
        return false;
    };
    w == expected_size && h == expected_size
}

fn unique_temp_path(final_path: &Path) -> PathBuf {
    let dir = final_path.parent().unwrap_or_else(|| Path::new("."));
    let name = final_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "derivative".to_owned());
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    dir.join(format!(".{name}.{pid}.{counter}.tmp"))
}

/// The temp must be a direct child of `artwork/` and have the exact
/// `.{name}.{pid}.{counter}.tmp` shape.
pub(crate) fn is_temp_artwork_file(relative: &str) -> bool {
    let Some(filename) = relative.strip_prefix("artwork/") else {
        return false;
    };
    !filename.is_empty() && !filename.contains('/') && matches_temp_artwork_filename(filename)
}

/// Keeping this next to the writer prevents the orphan scanner from granting
/// a grace period to unrelated hidden files.
pub(crate) fn matches_temp_artwork_filename(filename: &str) -> bool {
    let Some(stem) = filename
        .strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    let mut parts = stem.rsplitn(3, '.');
    let Some(counter) = parts.next() else {
        return false;
    };
    let Some(pid) = parts.next() else {
        return false;
    };
    let Some(name) = parts.next() else {
        return false;
    };
    !name.is_empty() && pid.parse::<u32>().is_ok() && counter.parse::<u64>().is_ok()
}

/// Only the content-addressed filenames produced by
/// `derivative_relative_path` are accepted, so a corrupt database cannot turn
/// an artwork read/upload into an arbitrary file read inside the library.
pub(crate) fn resolve_artwork_path(
    library: &LibraryRoot,
    relative: &str,
) -> Result<(PathBuf, ArtworkSize)> {
    resolve_artwork_path_inner(library, relative, true)
}

/// Read paths and integrity audits use this variant so inspection never
/// mutates the library merely because an expected derivative directory is
/// absent.
pub(crate) fn resolve_existing_artwork_path(
    library: &LibraryRoot,
    relative: &str,
) -> Result<(PathBuf, ArtworkSize)> {
    resolve_artwork_path_inner(library, relative, false)
}

fn resolve_artwork_path_inner(
    library: &LibraryRoot,
    relative: &str,
    create_artwork_directory: bool,
) -> Result<(PathBuf, ArtworkSize)> {
    let (parsed, size) = parse_artwork_relative_path(relative)?;

    let root_canonical = library.root().canonicalize().with_context(|| {
        format!(
            "failed to canonicalize library root {}",
            library.root().display()
        )
    })?;
    let artwork_path = root_canonical.join(ARTWORK_DIRECTORY);
    if create_artwork_directory {
        fs::create_dir_all(&artwork_path).context("failed to ensure artwork directory")?;
    }
    let artwork_metadata =
        fs::symlink_metadata(&artwork_path).context("failed to inspect artwork directory")?;
    // `artwork/` must be the direct, real child created by LibraryRoot. This
    // rejects an attacker-controlled artwork-directory symlink before any
    // read, write, copy, or delete follows it outside the library root.
    if artwork_metadata.file_type().is_symlink() || !artwork_metadata.is_dir() {
        anyhow::bail!("artwork directory must not be a symlink");
    }

    let target = root_canonical.join(&parsed);
    let target_parent = target.parent().context("artwork path has no parent")?;
    if target_parent != artwork_path {
        anyhow::bail!("artwork path escapes artwork directory");
    }

    Ok((target, size))
}

/// Parse a recorded derivative path. It must be exactly one of the two
/// content-addressed names generated by this module:
/// `artwork/thumb_<64 lowercase hex>_80.webp` or
/// `artwork/preview_<64 lowercase hex>_256.webp`.
fn parse_artwork_relative_path(relative: &str) -> Result<(PathBuf, ArtworkSize)> {
    let filename = relative
        .strip_prefix("artwork/")
        .context("artwork path must start with artwork/")?;
    if filename.is_empty() || filename.contains('/') || filename.contains('\\') {
        anyhow::bail!("artwork path must contain exactly one filename");
    }
    for size in [ArtworkSize::Thumb, ArtworkSize::Preview] {
        let Some(digest) = filename
            .strip_prefix(size.filename_prefix())
            .and_then(|name| name.strip_suffix(size.filename_suffix()))
        else {
            continue;
        };
        let is_lower_hex_digest = digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if is_lower_hex_digest {
            return Ok((PathBuf::from(relative), size));
        }
    }

    anyhow::bail!("artwork filename is not a recognized derivative name")
}

pub fn read_artwork_derivative(
    library: &LibraryRoot,
    relative_path: &str,
    expected_size: u32,
) -> Result<Option<Vec<u8>>> {
    let (abs, recorded_size) = resolve_existing_artwork_path(library, relative_path)?;
    if recorded_size.expected_dimension() != expected_size {
        return Ok(None);
    }
    if !validate_derivative_file(&abs, expected_size) {
        return Ok(None);
    }
    let bytes = fs::read(&abs)
        .with_context(|| format!("failed to read artwork derivative at {}", abs.display()))?;
    Ok(Some(bytes))
}

/// Delete a derivative file from disk only if no other song row references it.
/// Returns `true` if the file was removed (or already absent and unreferenced),
/// `false` if it was kept because another song row still references it.
pub fn delete_artwork_derivative_if_unreferenced(
    connection: &rusqlite::Connection,
    library: &LibraryRoot,
    relative_path: &str,
) -> Result<bool> {
    let ref_count = crate::cache::count_artwork_path_references(connection, relative_path)
        .context("failed to count artwork path references")?;
    if ref_count > 0 {
        return Ok(false);
    }
    let (abs, _) = resolve_artwork_path(library, relative_path)?;
    if fs::symlink_metadata(&abs).is_ok() {
        fs::remove_file(&abs)
            .with_context(|| format!("failed to remove artwork derivative at {}", abs.display()))?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache;
    use crate::library_root::LibraryRoot;
    use image::{ImageBuffer, Rgba};

    fn test_library() -> (tempfile::TempDir, LibraryRoot) {
        let tmp = tempfile::tempdir().unwrap();
        let lib = LibraryRoot::create(tmp.path().join("TestLib").as_path()).unwrap();
        (tmp, lib)
    }

    fn make_test_jpeg(size: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(size, size, Rgba([0, 255, 0, 255]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut buf),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        buf
    }

    #[test]
    fn generates_thumb_and_preview_derivatives() {
        let (_tmp, lib) = test_library();
        let original = make_test_jpeg(500);

        let derivatives = generate_artwork_derivatives(&lib, &original).unwrap();

        assert!(derivatives.thumb_path.starts_with("artwork/thumb_"));
        assert!(derivatives.thumb_path.ends_with("_80.webp"));
        assert!(derivatives.preview_path.starts_with("artwork/preview_"));
        assert!(derivatives.preview_path.ends_with("_256.webp"));

        // Verify files exist and have correct dimensions.
        let thumb_abs = lib.resolve(&derivatives.thumb_path);
        let preview_abs = lib.resolve(&derivatives.preview_path);
        assert!(thumb_abs.exists());
        assert!(preview_abs.exists());

        let file = std::fs::File::open(&thumb_abs).unwrap();
        let reader = ImageReader::new(std::io::BufReader::new(file))
            .with_guessed_format()
            .unwrap();
        assert_eq!(reader.format(), Some(image::ImageFormat::WebP));
        let (w, h) = reader.into_dimensions().unwrap();
        assert_eq!(w, 80);
        assert_eq!(h, 80);

        let file = std::fs::File::open(&preview_abs).unwrap();
        let reader = ImageReader::new(std::io::BufReader::new(file))
            .with_guessed_format()
            .unwrap();
        assert_eq!(reader.format(), Some(image::ImageFormat::WebP));
        let (w, h) = reader.into_dimensions().unwrap();
        assert_eq!(w, 256);
        assert_eq!(h, 256);
    }

    #[test]
    fn same_cover_produces_same_digest_filename() {
        let (_tmp, lib) = test_library();
        let original = make_test_jpeg(300);

        let d1 = generate_artwork_derivatives(&lib, &original).unwrap();
        let d2 = generate_artwork_derivatives(&lib, &original).unwrap();

        assert_eq!(d1.thumb_path, d2.thumb_path);
        assert_eq!(d1.preview_path, d2.preview_path);
    }

    #[test]
    fn different_covers_produce_different_digests() {
        let (_tmp, lib) = test_library();
        let original1 = make_test_jpeg(300);
        let original2 = make_test_jpeg(400);

        let d1 = generate_artwork_derivatives(&lib, &original1).unwrap();
        let d2 = generate_artwork_derivatives(&lib, &original2).unwrap();

        assert_ne!(d1.thumb_path, d2.thumb_path);
        assert_ne!(d1.preview_path, d2.preview_path);
    }

    #[test]
    fn rejects_oversized_input_bytes() {
        let (_tmp, lib) = test_library();
        let huge = vec![0u8; MAX_INPUT_BYTES + 1];
        let result = generate_artwork_derivatives(&lib, &huge);
        assert!(result.is_err());
    }

    #[test]
    fn parse_artwork_relative_path_rejects_absolute() {
        assert!(parse_artwork_relative_path("/artwork/thumb_x.webp").is_err());
    }

    #[test]
    fn parse_artwork_relative_path_rejects_traversal() {
        assert!(parse_artwork_relative_path("artwork/../thumb_x.webp").is_err());
        assert!(parse_artwork_relative_path("artwork/../../etc/passwd").is_err());
    }

    #[test]
    fn parse_artwork_relative_path_rejects_wrong_prefix() {
        assert!(parse_artwork_relative_path("media/thumb_x.webp").is_err());
        assert!(parse_artwork_relative_path("thumb_x.webp").is_err());
    }

    #[test]
    fn parse_artwork_relative_path_rejects_nested_separators() {
        assert!(parse_artwork_relative_path("artwork/a/b.webp").is_err());
    }

    #[test]
    fn parse_artwork_relative_path_accepts_valid() {
        let digest = "a".repeat(64);
        let relative = format!("artwork/thumb_{digest}_80.webp");
        let (path, size) = parse_artwork_relative_path(&relative).unwrap();
        assert_eq!(path, Path::new(&relative));
        assert_eq!(size, ArtworkSize::Thumb);
    }

    #[test]
    fn parse_artwork_relative_path_rejects_unrecognized_or_noncanonical_filenames() {
        let digest = "a".repeat(64);
        assert!(parse_artwork_relative_path(&format!("artwork/cover_{digest}.webp")).is_err());
        assert!(
            parse_artwork_relative_path(&format!("artwork/thumb_{}_80.webp", "A".repeat(64)))
                .is_err()
        );
        assert!(parse_artwork_relative_path("artwork/thumb_abc123_80.webp").is_err());
    }

    #[test]
    fn read_artwork_derivative_returns_none_for_missing() {
        let (_tmp, lib) = test_library();
        let missing = format!("artwork/thumb_{}_80.webp", "0".repeat(64));
        let result = read_artwork_derivative(&lib, &missing, 80).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn read_artwork_derivative_returns_bytes_for_valid() {
        let (_tmp, lib) = test_library();
        let original = make_test_jpeg(200);
        let derivatives = generate_artwork_derivatives(&lib, &original).unwrap();

        let bytes = read_artwork_derivative(&lib, &derivatives.thumb_path, 80).unwrap();
        assert!(bytes.is_some());
        assert!(!bytes.unwrap().is_empty());
    }

    #[test]
    fn read_artwork_derivative_returns_none_for_wrong_dimensions() {
        let (_tmp, lib) = test_library();
        let original = make_test_jpeg(200);
        let derivatives = generate_artwork_derivatives(&lib, &original).unwrap();

        // Request preview size but pass thumb path.
        let bytes = read_artwork_derivative(&lib, &derivatives.thumb_path, 256).unwrap();
        assert!(bytes.is_none());
    }

    #[test]
    fn jpeg_masquerading_as_webp_is_rejected_and_replaced() {
        let (_tmp, lib) = test_library();
        let original = make_test_jpeg(200);
        let digest = cover_sha256(&original);
        let thumb_rel = derivative_relative_path(ArtworkSize::Thumb, &digest);
        let (thumb_abs, _) = resolve_artwork_path(&lib, &thumb_rel).unwrap();

        // A valid 80x80 JPEG with a `.webp` name must not pass derivative
        // validation or be published/read as a WebP thumbnail.
        fs::write(&thumb_abs, make_test_jpeg(80)).unwrap();
        assert!(!validate_derivative_file(&thumb_abs, THUMB_SIZE));
        assert!(read_artwork_derivative(&lib, &thumb_rel, THUMB_SIZE)
            .unwrap()
            .is_none());

        generate_artwork_derivatives(&lib, &original).unwrap();
        assert!(validate_derivative_file(&thumb_abs, THUMB_SIZE));
    }

    #[cfg(unix)]
    #[test]
    fn generation_replaces_a_derivative_symlink_without_following_it() {
        use std::os::unix::fs::symlink;

        let (tmp, lib) = test_library();
        let original = make_test_jpeg(200);
        let digest = cover_sha256(&original);
        let thumb_rel = derivative_relative_path(ArtworkSize::Thumb, &digest);
        let (thumb_abs, _) = resolve_artwork_path(&lib, &thumb_rel).unwrap();
        let outside = tmp.path().join("outside.webp");
        fs::write(&outside, b"not an image").unwrap();
        symlink(&outside, &thumb_abs).unwrap();

        assert!(!validate_derivative_file(&thumb_abs, THUMB_SIZE));
        assert!(read_artwork_derivative(&lib, &thumb_rel, THUMB_SIZE)
            .unwrap()
            .is_none());

        generate_artwork_derivatives(&lib, &original).unwrap();
        assert!(!fs::symlink_metadata(&thumb_abs)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(validate_derivative_file(&thumb_abs, THUMB_SIZE));
        assert_eq!(fs::read(&outside).unwrap(), b"not an image");
    }

    #[test]
    fn delete_derivative_if_unreferenced() {
        let (_tmp, lib) = test_library();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        cache::apply_migrations(&conn).unwrap();

        let original = make_test_jpeg(200);
        let derivatives = generate_artwork_derivatives(&lib, &original).unwrap();

        // No song references the path, so it should be deleted.
        delete_artwork_derivative_if_unreferenced(&conn, &lib, &derivatives.thumb_path).unwrap();
        assert!(!lib.resolve(&derivatives.thumb_path).exists());
    }

    #[test]
    fn delete_derivative_keeps_file_when_referenced() {
        let (_tmp, lib) = test_library();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        cache::apply_migrations(&conn).unwrap();

        let original = make_test_jpeg(200);
        let derivatives = generate_artwork_derivatives(&lib, &original).unwrap();

        // Insert a song that references the thumb path.
        let song = crate::library::Song {
            hash: "test-song".to_owned(),
            file_path: Some("media/test.mp3".to_owned()),
            cdg_path: None,
            media_g_container: None,
            instrumental: false,
            language: None,
            audio_source_kind: "original".to_owned(),
            title: Some("Test".to_owned()),
            artist: None,
            album: None,
            duration_ms: 1000,
            cover_art: Some(original.clone()),
            has_cover_art: true,
            imported_at: 0,
            original_ext: Some("mp3".to_owned()),
        };
        cache::upsert_song(&conn, &song).unwrap();
        cache::update_artwork_derivative_paths(
            &conn,
            "test-song",
            Some(&derivatives.thumb_path),
            Some(&derivatives.preview_path),
        )
        .unwrap();

        // Path is referenced, so file should NOT be deleted.
        delete_artwork_derivative_if_unreferenced(&conn, &lib, &derivatives.thumb_path).unwrap();
        assert!(lib.resolve(&derivatives.thumb_path).exists());
    }

    #[test]
    fn cover_sha256_is_deterministic() {
        let bytes = b"test cover art bytes";
        let d1 = cover_sha256(bytes);
        let d2 = cover_sha256(bytes);
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }

    #[test]
    fn temp_file_naming_is_unique() {
        let p1 = unique_temp_path(Path::new("artwork/thumb_abc_80.webp"));
        let p2 = unique_temp_path(Path::new("artwork/thumb_abc_80.webp"));
        assert_ne!(p1, p2);
        assert!(p1.to_string_lossy().contains(".tmp"));
        assert!(p2.to_string_lossy().contains(".tmp"));
    }

    #[test]
    fn is_temp_artwork_file_matches_writer_convention() {
        // Valid writer-produced temp paths: .{name}.{pid}.{counter}.tmp
        assert!(is_temp_artwork_file("artwork/.thumb_abc.webp.12345.0.tmp"));
        assert!(is_temp_artwork_file("artwork/.preview_xyz.99999.42.tmp"));
        assert!(is_temp_artwork_file("artwork/.derivative.1.0.tmp"));
    }

    #[test]
    fn is_temp_artwork_file_rejects_invalid_paths() {
        // Not under artwork/
        assert!(!is_temp_artwork_file("media/.foo.123.0.tmp"));
        // No leading dot
        assert!(!is_temp_artwork_file("artwork/thumb.123.0.tmp"));
        // No .tmp suffix
        assert!(!is_temp_artwork_file("artwork/.thumb.123.0.bin"));
        // Missing pid/counter (too few components)
        assert!(!is_temp_artwork_file("artwork/.foo.tmp"));
        assert!(!is_temp_artwork_file("artwork/.foo.bar.tmp"));
        // Non-numeric pid/counter
        assert!(!is_temp_artwork_file("artwork/.foo.abc.def.tmp"));
        assert!(!is_temp_artwork_file("artwork/.thumb.webp.abc.0.tmp"));
        assert!(!is_temp_artwork_file("artwork/.thumb.webp.123.def.tmp"));
        // Nested paths — writer never places temp files in subdirectories
        assert!(!is_temp_artwork_file("artwork/sub/.foo.123.0.tmp"));
        assert!(!is_temp_artwork_file("artwork/a/b/.thumb.1.0.tmp"));
        // Arbitrary hidden .tmp files not matching the writer convention
        assert!(!is_temp_artwork_file("artwork/.notes.tmp"));
        assert!(!is_temp_artwork_file("artwork/.tmp"));
    }

    #[test]
    fn matches_temp_artwork_filename_unit_cases() {
        assert!(matches_temp_artwork_filename(
            ".thumb_abc_80.webp.12345.0.tmp"
        ));
        assert!(matches_temp_artwork_filename(".x.1.0.tmp"));
        assert!(!matches_temp_artwork_filename(
            "thumb_abc_80.webp.12345.0.tmp"
        ));
        assert!(!matches_temp_artwork_filename(
            ".thumb_abc_80.webp.12345.0.bin"
        ));
        assert!(!matches_temp_artwork_filename(".tmp"));
    }
}
