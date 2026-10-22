//! A watch's photos as thumbnails: read off the screen's thread, turned
//! the way the camera says, made small, kept on disk in this computer's
//! cache folder so the next start shows them at once, and handed to egui
//! as textures. JPEG, PNG and TIFF are read with `image`; HEIC, as an
//! iPhone takes them, with `heic-rs`.

use eframe::egui;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::SystemTime;

/// The longest side of a thumbnail, in pixels: sharp at the photo strip's
/// size on a high-density screen.
pub const THUMB_PX: u32 = 256;

/// The photo at `path`, made no larger than `max` pixels on its longer
/// side, the right way up.
pub fn decode(path: &Path, max: u32) -> Result<egui::ColorImage, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let img = if matches!(ext.as_str(), "heic" | "heif") {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        // `heic-rs` turns and mirrors it as the file says.
        let opts = heic_rs::DecodeOptions::default().with_layout(heic_rs::PixelLayout::Rgba8);
        let i = heic_rs::decode(&bytes, &opts).map_err(|e| e.to_string())?;
        let rgba = image::RgbaImage::from_raw(i.width, i.height, i.data)
            .ok_or("the picture's size doesn't match its pixels")?;
        image::DynamicImage::ImageRgba8(rgba)
    } else {
        use image::ImageDecoder;
        let mut d = image::ImageReader::open(path)
            .and_then(|r| r.with_guessed_format())
            .map_err(|e| e.to_string())?
            .into_decoder()
            .map_err(|e| e.to_string())?;
        let turn = d
            .orientation()
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let mut img = image::DynamicImage::from_decoder(d).map_err(|e| e.to_string())?;
        img.apply_orientation(turn);
        img
    };
    // Made smaller, never larger.
    let small = if img.width() > max || img.height() > max {
        img.thumbnail(max, max)
    } else {
        img
    }
    .to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [small.width() as usize, small.height() as usize],
        small.as_raw(),
    ))
}

/// Where thumbnails are kept between starts: the system's cache folder for
/// the app, never the library (which may be read-only, or on another
/// computer next time).
fn cache_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return Some(std::env::temp_dir().join("timegrapher-thumbnails-test"));
    }
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(target_os = "windows") {
        env("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Caches"))
    } else {
        env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))
    };
    base.map(|b| b.join("timegrapher").join("thumbnails"))
}

/// The cached thumbnail's file for `path` as it is now: a changed photo
/// gets a new one.
fn cached(dir: &Path, path: &Path, changed: Option<SystemTime>, len: u64) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, changed, len, THUMB_PX).hash(&mut h);
    dir.join(format!("{:016x}.png", h.finish()))
}

/// The thumbnail for `path`: from the cache when it is there, else read
/// and then cached (a cache that can't be written is left out).
fn thumbnail(path: &Path) -> Result<egui::ColorImage, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let file = cache_dir().map(|d| cached(&d, path, meta.modified().ok(), meta.len()));
    if let Some(f) = &file {
        if let Ok(img) = image::open(f) {
            let img = img.to_rgba8();
            return Ok(egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            ));
        }
    }
    let img = decode(path, THUMB_PX)?;
    if let Some(f) = file {
        let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
        let [w, h] = img.size;
        if let Some(rgba) = image::RgbaImage::from_raw(w as u32, h as u32, img.as_raw().to_vec()) {
            let _ = rgba.save(&f);
        }
    }
    Ok(img)
}

/// A photo's thumbnail as the screen has it.
pub enum Thumb {
    Loading,
    Ready(egui::TextureHandle),
    /// It couldn't be read, and why.
    Failed(String),
}

type Done = (PathBuf, Result<egui::ColorImage, String>);

/// The thumbnails asked for, each read once by a thread of its own kind,
/// one at a time.
#[derive(Default)]
pub struct Thumbs {
    shown: HashMap<PathBuf, Thumb>,
    work: Option<(Sender<PathBuf>, Receiver<Done>)>,
}

impl Thumbs {
    /// The thumbnail of `path`, asked for the first time it is wanted.
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> &Thumb {
        self.collect(ctx);
        if !self.shown.contains_key(path) {
            let (tx, _) = self.work.get_or_insert_with(|| {
                let (tx, jobs) = mpsc::channel::<PathBuf>();
                let (done, rx) = mpsc::channel::<Done>();
                let ctx = ctx.clone();
                std::thread::Builder::new()
                    .name("thumbnails".into())
                    .spawn(move || {
                        for p in jobs {
                            let t = thumbnail(&p);
                            if done.send((p, t)).is_err() {
                                break;
                            }
                            ctx.request_repaint();
                        }
                    })
                    .expect("a thread for thumbnails");
                (tx, rx)
            });
            let _ = tx.send(path.to_path_buf());
            self.shown.insert(path.to_path_buf(), Thumb::Loading);
        }
        &self.shown[path]
    }

    /// Take in the thumbnails read since the last frame.
    fn collect(&mut self, ctx: &egui::Context) {
        let Some((_, rx)) = &self.work else {
            return;
        };
        for (p, t) in rx.try_iter() {
            let thumb = match t {
                Ok(img) => Thumb::Ready(ctx.load_texture(
                    format!("thumb {}", p.display()),
                    img,
                    egui::TextureOptions::LINEAR,
                )),
                Err(e) => Thumb::Failed(e),
            };
            self.shown.insert(p, thumb);
        }
    }

    /// Forget a photo's thumbnail (one deleted, or replaced).
    pub fn forget(&mut self, path: &Path) {
        self.shown.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("photos")
            .join(name)
    }

    /// Red on the left, blue on the right, as the test pictures are drawn.
    fn red_then_blue(img: &egui::ColorImage) -> bool {
        let [w, h] = img.size;
        let at = |x: usize| img.pixels[(h / 2) * w + x];
        let (l, r) = (at(w / 8), at(w - 1 - w / 8));
        l.r() > 150 && l.b() < 100 && r.b() > 150 && r.r() < 100
    }

    #[test]
    fn photos_are_read_small_and_the_right_way_up() {
        // PNG and HEIC (as an iPhone takes them), wider than tall.
        let png = decode(&data("plain.png"), THUMB_PX).unwrap();
        assert_eq!(png.size, [80, 40]);
        assert!(red_then_blue(&png));
        let heic = decode(&data("plain.heic"), THUMB_PX).unwrap();
        assert_eq!(heic.size, [96, 64]);
        assert!(red_then_blue(&heic));
        // Made smaller, keeping its shape.
        let small = decode(&data("plain.heic"), 48).unwrap();
        assert_eq!(small.size, [48, 32]);
        // A JPEG taken with the camera turned is stored 80 by 40, red then
        // blue, and marked to be turned a quarter clockwise: shown 40 by 80,
        // red above blue.
        let jpg = decode(&data("turned.jpg"), THUMB_PX).unwrap();
        assert_eq!(jpg.size, [40, 80]);
        let [w, h] = jpg.size;
        let top = jpg.pixels[(h / 8) * w + w / 2];
        let bottom = jpg.pixels[(h - 1 - h / 8) * w + w / 2];
        assert!(top.r() > 150 && top.b() < 100, "{top:?}");
        assert!(bottom.b() > 150 && bottom.r() < 100, "{bottom:?}");
        // Not a picture: an error, not a blank.
        let not = std::env::temp_dir().join(format!("tg-not-a-photo-{}.jpg", std::process::id()));
        std::fs::write(&not, b"not a photo").unwrap();
        assert!(decode(&not, THUMB_PX).is_err());
        let _ = std::fs::remove_file(&not);
    }

    #[test]
    fn a_thumbnail_is_cached_and_made_again_when_the_photo_changes() {
        let dir = std::env::temp_dir().join(format!("tg-thumb-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("dial.png");
        std::fs::copy(data("plain.png"), &photo).unwrap();
        let first = thumbnail(&photo).unwrap();
        let m = std::fs::metadata(&photo).unwrap();
        let file = cached(&cache_dir().unwrap(), &photo, m.modified().ok(), m.len());
        assert!(file.exists());
        assert_eq!(thumbnail(&photo).unwrap().size, first.size);
        // Another picture under the same name: not the old thumbnail.
        std::fs::copy(data("turned.jpg"), &photo).unwrap();
        assert_eq!(thumbnail(&photo).unwrap().size, [40, 80]);
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
