//! Loading and saving images, the layered `.paint` project format, and the clipboard.

use crate::color::Rgba;
use crate::doc::{Document, Layer};
use crate::raster;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use tiny_skia::Pixmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Bmp,
    Gif,
    Tiff,
    WebP,
    Ico,
    Project,
}

impl Format {
    pub const SAVE_CHOICES: [Format; 8] =
        [Format::Png, Format::Jpeg, Format::Bmp, Format::Gif, Format::Tiff, Format::WebP, Format::Ico, Format::Project];

    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG picture",
            Format::Jpeg => "JPEG picture",
            Format::Bmp => "BMP picture",
            Format::Gif => "GIF picture",
            Format::Tiff => "TIFF picture",
            Format::WebP => "WEBP picture",
            Format::Ico => "ICO icon",
            Format::Project => "Paint project",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Format::Png => &["png"],
            Format::Jpeg => &["jpg", "jpeg", "jpe", "jfif"],
            Format::Bmp => &["bmp", "dib"],
            Format::Gif => &["gif"],
            Format::Tiff => &["tif", "tiff"],
            Format::WebP => &["webp"],
            Format::Ico => &["ico"],
            Format::Project => &["paint"],
        }
    }

    pub fn from_path(p: &Path) -> Option<Format> {
        let ext = p.extension()?.to_string_lossy().to_lowercase();
        Format::SAVE_CHOICES.into_iter().find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// Formats that cannot store an alpha channel (Paint warns that transparency is lost).
    pub fn supports_alpha(self) -> bool {
        !matches!(self, Format::Jpeg | Format::Bmp)
    }

    pub fn supports_layers(self) -> bool {
        self == Format::Project
    }
}

pub const OPEN_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "jpe", "jfif", "bmp", "dib", "gif", "tif", "tiff", "webp", "ico", "tga", "pnm", "qoi", "paint"];

/// Decode a picture file, honouring the EXIF orientation (photos from phones and cameras).
fn decode(path: &Path) -> Result<Pixmap> {
    use image::ImageDecoder;
    let invalid = || format!("{} is not a valid picture, or its format is not supported.", path.display());
    let mut decoder = image::ImageReader::open(path)
        .with_context(|| format!("Can't open {}", path.display()))?
        .with_guessed_format()?
        .into_decoder()
        .with_context(invalid)?;
    let (w, h) = decoder.dimensions();
    if w > crate::doc::MAX_DIM || h > crate::doc::MAX_DIM {
        bail!("The picture is too large ({w} × {h}).");
    }
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).with_context(invalid)?;
    img.apply_orientation(orientation);
    Ok(raster::from_image(&img.to_rgba8()))
}

pub fn load(path: &Path) -> Result<Document> {
    let mut doc = if Format::from_path(path) == Some(Format::Project) {
        load_project(path)?
    } else {
        Document::from_pixmap(decode(path)?)
    };
    doc.path = Some(path.to_path_buf());
    doc.file_size = std::fs::metadata(path).ok().map(|m| m.len());
    Ok(doc)
}

pub fn load_pixmap(path: &Path) -> Result<Pixmap> {
    if Format::from_path(path) == Some(Format::Project) {
        return Ok(load_project(path)?.flatten());
    }
    decode(path)
}

/// Write a file through a temporary sibling so a failed save never truncates the original.
fn write_atomically(path: &Path, write: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> Result<()>) -> Result<()> {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{}.unix-paint-tmp", std::process::id()));
    let tmp = path.with_file_name(name);
    let result = (|| {
        let file = std::fs::File::create(&tmp).with_context(|| format!("Can't create {}", path.display()))?;
        let mut w = std::io::BufWriter::new(file);
        write(&mut w)?;
        w.flush()?;
        // Keep the permissions of a file we are replacing.
        if let Ok(meta) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&tmp, meta.permissions());
        }
        std::fs::rename(&tmp, path).with_context(|| format!("Can't write {}", path.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Save the document. Flat formats save the flattened image.
pub fn save(doc: &Document, path: &Path, format: Format) -> Result<()> {
    if format == Format::Project {
        return save_project(doc, path);
    }
    let mut flat = doc.flatten();
    if doc.monochrome {
        raster::to_monochrome(&mut flat);
    }
    let rgba = raster::to_image(&flat);
    write_atomically(path, |mut w| {
        match format {
            Format::Jpeg | Format::Bmp => {
                // No alpha: composite onto white like Paint does.
                let rgb = image::DynamicImage::ImageRgba8(flatten_onto(&rgba, Rgba::WHITE)).to_rgb8();
                if format == Format::Jpeg {
                    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 92);
                    rgb.write_with_encoder(enc)?;
                } else {
                    rgb.write_to(&mut w, image::ImageFormat::Bmp)?;
                }
            }
            Format::Gif => rgba.write_to(&mut w, image::ImageFormat::Gif)?,
            Format::Tiff => rgba.write_to(&mut w, image::ImageFormat::Tiff)?,
            Format::WebP => rgba.write_to(&mut w, image::ImageFormat::WebP)?,
            Format::Ico => {
                let r = if rgba.width() > 256 || rgba.height() > 256 {
                    let s = 256.0 / rgba.width().max(rgba.height()) as f32;
                    image::imageops::resize(
                        &rgba,
                        ((rgba.width() as f32 * s) as u32).max(1),
                        ((rgba.height() as f32 * s) as u32).max(1),
                        image::imageops::FilterType::Lanczos3,
                    )
                } else {
                    rgba
                };
                r.write_to(&mut w, image::ImageFormat::Ico)?
            }
            Format::Png => {
                let enc = image::codecs::png::PngEncoder::new(&mut w);
                rgba.write_with_encoder(enc)?;
            }
            Format::Project => unreachable!(),
        }
        Ok(())
    })
}

fn flatten_onto(img: &image::RgbaImage, bg: Rgba) -> image::RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let a = p[3] as u32;
        let mix = |c: u8, b: u8| ((c as u32 * a + b as u32 * (255 - a)) / 255) as u8;
        *p = image::Rgba([mix(p[0], bg.r), mix(p[1], bg.g), mix(p[2], bg.b), 255]);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// .paint project: "UNXPAINT" magic, u32 version, u32 json length, JSON header, then each layer
// as u64 length + PNG bytes (bottom to top).

const MAGIC: &[u8; 8] = b"UNXPAINT";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct ProjectHeader {
    width: u32,
    height: u32,
    dpi: f32,
    background: Rgba,
    background_visible: bool,
    active: usize,
    layers: Vec<LayerHeader>,
}

#[derive(Serialize, Deserialize)]
struct LayerHeader {
    name: String,
    visible: bool,
}

pub fn save_project(doc: &Document, path: &Path) -> Result<()> {
    let header = ProjectHeader {
        width: doc.width,
        height: doc.height,
        dpi: doc.dpi,
        background: doc.background,
        background_visible: doc.background_visible,
        active: doc.active,
        layers: doc.layers.iter().map(|l| LayerHeader { name: l.name.clone(), visible: l.visible }).collect(),
    };
    let json = serde_json::to_vec(&header)?;
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    for l in &doc.layers {
        let png = l.pixels.encode_png().map_err(|e| anyhow!("{e}"))?;
        out.extend_from_slice(&(png.len() as u64).to_le_bytes());
        out.extend_from_slice(&png);
    }
    write_atomically(path, |w| Ok(w.write_all(&out)?))
}

pub fn load_project(path: &Path) -> Result<Document> {
    let mut f = std::fs::File::open(path)?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    let bad = || anyhow!("{} is not a valid Paint project.", path.display());
    if data.len() < 16 || &data[..8] != MAGIC {
        return Err(bad());
    }
    let ver = u32::from_le_bytes(data[8..12].try_into()?);
    if ver > VERSION {
        bail!("This project was made by a newer version of Paint.");
    }
    let jl = u32::from_le_bytes(data[12..16].try_into()?) as usize;
    let json_end = 16usize.checked_add(jl).ok_or_else(bad)?;
    let header: ProjectHeader = serde_json::from_slice(data.get(16..json_end).ok_or_else(bad)?).map_err(|_| bad())?;
    let max = crate::doc::MAX_DIM;
    if header.width == 0 || header.height == 0 || header.width > max || header.height > max {
        return Err(bad());
    }
    let mut off = json_end;
    let mut layers = Vec::new();
    for lh in header.layers {
        let n = usize::try_from(u64::from_le_bytes(data.get(off..off + 8).ok_or_else(bad)?.try_into()?)).map_err(|_| bad())?;
        off += 8;
        let end = off.checked_add(n).ok_or_else(bad)?;
        let png = data.get(off..end).ok_or_else(bad)?;
        off = end;
        let mut pixels = Pixmap::decode_png(png).map_err(|_| bad())?;
        if pixels.width() != header.width || pixels.height() != header.height {
            let mut p = raster::new_pixmap(header.width, header.height);
            raster::put_rect(&mut p, &pixels, 0, 0);
            pixels = p;
        }
        layers.push(Layer { name: lh.name, visible: lh.visible, pixels: pixels.into() });
    }
    if layers.is_empty() {
        return Err(bad());
    }
    let mut doc = Document::new(header.width, header.height);
    doc.layers = layers;
    doc.active = header.active.min(doc.layers.len() - 1);
    doc.background = header.background;
    doc.background_visible = header.background_visible;
    doc.dpi = if header.dpi.is_finite() { header.dpi.clamp(1.0, 9600.0) } else { 96.0 };
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// Clipboard

//
// One `arboard::Clipboard` lives for the whole session. On X11 the copied data is served by
// this process, so a clipboard that is dropped right after `set_image` loses its contents
// unless a clipboard manager happens to be running.

thread_local! {
    static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
    /// The last copied picture, used when the system clipboard is unavailable.
    static LOCAL_COPY: std::cell::RefCell<Option<Pixmap>> = const { std::cell::RefCell::new(None) };
}

fn with_clipboard<T>(f: impl FnOnce(&mut arboard::Clipboard) -> std::result::Result<T, arboard::Error>) -> Result<T> {
    CLIPBOARD.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            *c = Some(arboard::Clipboard::new()?);
        }
        Ok(f(c.as_mut().unwrap())?)
    })
}

/// Release the clipboard before exiting (on X11 this hands the contents to the clipboard manager).
pub fn clipboard_shutdown() {
    CLIPBOARD.with(|c| c.borrow_mut().take());
}

pub fn clipboard_set_text(s: String) -> Result<()> {
    with_clipboard(|cb| cb.set_text(s))
}

pub fn clipboard_get_text() -> Result<String> {
    with_clipboard(|cb| cb.get_text())
}

pub fn clipboard_set_image(p: &Pixmap) -> Result<()> {
    let bytes = p.clone().take_demultiplied();
    let img = arboard::ImageData { width: p.width() as usize, height: p.height() as usize, bytes: bytes.into() };
    match with_clipboard(|cb| cb.set_image(img)) {
        Ok(()) => {
            LOCAL_COPY.with(|l| l.borrow_mut().take());
            Ok(())
        }
        Err(e) => {
            // Copy and paste keep working inside the app even without a system clipboard.
            log::warn!("system clipboard unavailable: {e}");
            LOCAL_COPY.with(|l| *l.borrow_mut() = Some(p.clone()));
            Ok(())
        }
    }
}

/// Turn a `file://` URI (as file managers put on the clipboard) or a plain path into a path.
fn path_from_clipboard_text(text: &str) -> Option<std::path::PathBuf> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let Some(rest) = line.strip_prefix("file://") else { return Some(line.into()) };
    // Skip an optional host ("file://localhost/…") and decode %XX escapes.
    let rest = &rest[rest.find('/')?..];
    let raw = rest.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (raw[i], raw.get(i + 1).copied().and_then(hex), raw.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                bytes.push((h * 16 + l) as u8);
                i += 3;
            }
            (b, _, _) => {
                bytes.push(b);
                i += 1;
            }
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned().into())
}

pub fn clipboard_get_image() -> Result<Option<Pixmap>> {
    let local = || LOCAL_COPY.with(|l| l.borrow().clone());
    match with_clipboard(|cb| cb.get_image()) {
        Ok(img) => {
            let rgba = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
                .ok_or_else(|| anyhow!("bad clipboard image"))?;
            Ok(Some(raster::from_image(&rgba)))
        }
        Err(e) => {
            // A copied file path is also accepted (file managers put URIs on the clipboard).
            if let Ok(text) = clipboard_get_text() {
                if let Some(path) = path_from_clipboard_text(&text).filter(|p| p.is_file()) {
                    return load_pixmap(&path).map(Some);
                }
            }
            match (local(), e.downcast_ref::<arboard::Error>()) {
                (Some(p), _) => Ok(Some(p)),
                (None, Some(arboard::Error::ContentNotAvailable)) => Ok(None),
                (None, _) => Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("unix-paint-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn project_roundtrip() {
        let mut d = Document::new(16, 8);
        d.layers[0].pixels_mut().pixels_mut()[3] = Rgba::rgb(1, 2, 3).premul();
        d.layers.push(Layer::new("Top", 16, 8));
        d.layers[1].visible = false;
        d.background = Rgba::rgb(9, 9, 9);
        let p = tmpdir().join("a.paint");
        save(&d, &p, Format::Project).unwrap();
        let e = load(&p).unwrap();
        assert_eq!(e.layers.len(), 2);
        assert_eq!(e.layers[1].name, "Top");
        assert!(!e.layers[1].visible);
        assert_eq!(e.background, Rgba::rgb(9, 9, 9));
        assert_eq!(raster::get_pixel(&e.layers[0].pixels, 3, 0), Some(Rgba::rgb(1, 2, 3)));
    }

    #[test]
    fn malformed_project_is_rejected_not_panicking() {
        let dir = tmpdir();
        let p = dir.join("bad.paint");
        for header in [r#"{"width":0,"height":8,"dpi":96.0,"background":{"r":0,"g":0,"b":0,"a":255},"background_visible":true,"active":0,"layers":[{"name":"a","visible":true}]}"#,
                       r#"{"width":4000000000,"height":8,"dpi":96.0,"background":{"r":0,"g":0,"b":0,"a":255},"background_visible":true,"active":0,"layers":[{"name":"a","visible":true}]}"#] {
            let mut data = MAGIC.to_vec();
            data.extend_from_slice(&VERSION.to_le_bytes());
            data.extend_from_slice(&(header.len() as u32).to_le_bytes());
            data.extend_from_slice(header.as_bytes());
            data.extend_from_slice(&u64::MAX.to_le_bytes());
            std::fs::write(&p, &data).unwrap();
            assert!(load(&p).is_err());
        }
    }

    #[test]
    fn clipboard_uris_are_decoded() {
        let p = |s| path_from_clipboard_text(s).unwrap();
        assert_eq!(p("file:///tmp/my%20pic.png\n"), std::path::PathBuf::from("/tmp/my pic.png"));
        assert_eq!(p("file://localhost/tmp/a.png"), std::path::PathBuf::from("/tmp/a.png"));
        assert_eq!(p("/tmp/100%.png"), std::path::PathBuf::from("/tmp/100%.png"));
    }

    #[test]
    fn failed_save_leaves_no_temp_file() {
        let dir = tmpdir().join("nosuchdir");
        let d = Document::new(4, 4);
        assert!(save(&d, &dir.join("x.png"), Format::Png).is_err());
        // A directory of its own: other tests save into the shared one concurrently.
        let dir = tmpdir().join("atomic");
        std::fs::create_dir_all(&dir).unwrap();
        save(&d, &dir.join("clean.png"), Format::Png).unwrap();
        assert!(std::fs::read_dir(&dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("unix-paint-tmp")));
    }

    #[test]
    fn flat_formats_roundtrip_size() {
        let mut d = Document::new(20, 10);
        d.layers[0].pixels_mut().fill(Rgba::rgb(200, 10, 10).to_skia());
        for f in [Format::Png, Format::Jpeg, Format::Bmp, Format::Gif, Format::Tiff, Format::WebP] {
            let p = tmpdir().join(format!("x.{}", f.extensions()[0]));
            save(&d, &p, f).unwrap();
            let e = load(&p).unwrap_or_else(|err| panic!("{f:?}: {err}"));
            assert_eq!((e.width, e.height), (20, 10), "{f:?}");
            let c = raster::get_pixel(&e.layers[0].pixels, 5, 5).unwrap();
            assert!((c.r as i32 - 200).abs() < 8 && c.g < 30, "{f:?} {c:?}");
        }
    }
}
