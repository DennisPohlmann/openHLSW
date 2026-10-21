//! Build script:
//! * packs the game icons in `assets/games/*.ico` into two texture atlases: 16x16 cells for
//!   normal screens and 32x32 cells for high-DPI screens,
//! * prepares the window icon from `assets/openhlsw.ico`,
//! * embeds `assets/openhlsw.ico` as the .exe icon on Windows.

use std::env;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

const CELL: u32 = 32;
/// Game icons are shown at 16x16 screen pixels; this atlas holds them at exactly that size
/// (the icons' own 16x16 pictures where they have one).
const SMALL: u32 = 16;
const COLUMNS: u32 = 16;

fn main() {
    println!("cargo:rerun-if-changed=assets/games");
    println!("cargo:rerun-if-changed=assets/openhlsw.ico");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    // --- game icon atlases (16 px for normal screens, 32 px for high-DPI ones) --------
    let mut paths: Vec<PathBuf> = fs::read_dir("assets/games")
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    paths.sort();
    let paths: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .map(str::to_lowercase)
                .as_deref()
                == Some("ico")
        })
        .collect();
    let mut names: Vec<String> = Vec::new();
    let mut cells: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for path in &paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let name = stem.strip_prefix("GameType_").unwrap_or(stem).to_string();
        match (load_game_icon(path, CELL), load_game_icon(path, SMALL)) {
            (Some(large), Some(small)) => {
                names.push(name);
                cells.push((small, large));
            }
            _ => println!("cargo:warning=could not decode {}", path.display()),
        }
    }
    for (file, size) in [("game_icons16.png", SMALL), ("game_icons.png", CELL)] {
        let rows = (cells.len() as u32).div_ceil(COLUMNS).max(1);
        let (w, h) = (size * COLUMNS, size * rows);
        let mut atlas = vec![0u8; (w * h * 4) as usize];
        for (i, (small, large)) in cells.iter().enumerate() {
            let rgba = if size == SMALL { small } else { large };
            let (cx, cy) = ((i as u32 % COLUMNS) * size, (i as u32 / COLUMNS) * size);
            for y in 0..size {
                let src = (y * size * 4) as usize;
                let dst = (((cy + y) * w + cx) * 4) as usize;
                atlas[dst..dst + (size * 4) as usize]
                    .copy_from_slice(&rgba[src..src + (size * 4) as usize]);
            }
        }
        write_png(&out.join(file), w, h, &atlas);
    }
    fs::write(out.join("game_icons.txt"), names.join(",")).expect("write icon index");

    // --- window icon (64x64 RGBA) -------------------------------------------
    let app =
        load_icon(Path::new("assets/openhlsw.ico"), 64).unwrap_or_else(|| vec![0; 64 * 64 * 4]);
    fs::write(out.join("app_icon.rgba"), app).expect("write app icon");

    // --- .exe icon on Windows -----------------------------------------------
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && Path::new("assets/openhlsw.ico").exists()
    {
        // The source icon stores every size as PNG. Explorer, the Start menu and the taskbar
        // only read PNG reliably for 256 px, so the smaller sizes are embedded as classic
        // bitmaps (otherwise Windows shows a blank white icon there).
        let icon = bitmap_icon(Path::new("assets/openhlsw.ico"), &out.join("openhlsw.ico"))
            .unwrap_or_else(|| PathBuf::from("assets/openhlsw.ico"));
        println!("cargo:rerun-if-changed=assets/openhlsw.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(&icon.to_string_lossy());
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the program icon: {e}");
        }
    }
}

/// Copies an .ico with all entries below 256 px stored as bitmaps (BMP) instead of PNG.
fn bitmap_icon(src: &Path, dst: &Path) -> Option<PathBuf> {
    let dir = ico::IconDir::read(File::open(src).ok()?).ok()?;
    let mut out = ico::IconDir::new(ico::ResourceType::Icon);
    for entry in dir.entries() {
        let image = entry.decode().ok()?;
        let encoded = if image.width() >= 256 {
            ico::IconDirEntry::encode_as_png(&image)
        } else {
            ico::IconDirEntry::encode_as_bmp(&image)
        };
        out.add_entry(encoded.ok()?);
    }
    out.write(File::create(dst).ok()?).ok()?;
    Some(dst.to_path_buf())
}

/// Game icon for the atlas. Uses the entry of the right size when its picture fills the
/// canvas; otherwise (icons whose logo sits small in a big transparent canvas) the largest
/// entry, cropped to the visible part, so every icon fills its cell.
fn load_game_icon(path: &Path, size: u32) -> Option<Vec<u8>> {
    let dir = ico::IconDir::read(File::open(path).ok()?).ok()?;
    let entries = dir.entries();
    if let Some(exact) = entries
        .iter()
        .filter(|e| e.width() == size && e.height() == size)
        .max_by_key(|e| e.bits_per_pixel())
        && let Ok(image) = exact.decode()
        && let Some((x0, y0, x1, y1)) = alpha_bounds(image.rgba_data(), size, size)
        && (x1 - x0).max(y1 - y0) * 10 >= size * 8
    {
        return Some(image.rgba_data().to_vec());
    }
    let largest = entries
        .iter()
        .max_by_key(|e| (e.width(), e.bits_per_pixel()))?;
    let image = largest.decode().ok()?;
    let (w, h) = (image.width(), image.height());
    let data = image.rgba_data();
    let Some((x0, y0, x1, y1)) = alpha_bounds(data, w, h) else {
        return Some(resize(data, w, h, size));
    };
    // Square crop around the visible part, kept inside the image.
    let side = (x1 - x0).max(y1 - y0).max(1);
    let cx = (x0 + x1) / 2;
    let cy = (y0 + y1) / 2;
    let left = cx.saturating_sub(side / 2).min(w.saturating_sub(side));
    let top = cy.saturating_sub(side / 2).min(h.saturating_sub(side));
    let side = side.min(w).min(h);
    let mut cropped = vec![0u8; (side * side * 4) as usize];
    for y in 0..side {
        let src = (((top + y) * w + left) * 4) as usize;
        let dst = (y * side * 4) as usize;
        cropped[dst..dst + (side * 4) as usize]
            .copy_from_slice(&data[src..src + (side * 4) as usize]);
    }
    Some(resize(&cropped, side, side, size))
}

/// Bounding box (x0, y0, x1, y1; end exclusive) of the pixels that are not (almost)
/// transparent.
fn alpha_bounds(data: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if data[((y * w + x) * 4 + 3) as usize] > 16 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

/// Decodes the best entry of an .ico file and scales it to `size` x `size` RGBA.
fn load_icon(path: &Path, size: u32) -> Option<Vec<u8>> {
    let dir = ico::IconDir::read(File::open(path).ok()?).ok()?;
    let entries = dir.entries();
    // exact size if present, otherwise the smallest larger one, otherwise the largest
    let entry = entries
        .iter()
        .filter(|e| e.width() == size)
        .max_by_key(|e| e.bits_per_pixel())
        .or_else(|| {
            entries
                .iter()
                .filter(|e| e.width() > size)
                .min_by_key(|e| e.width())
        })
        .or_else(|| entries.iter().max_by_key(|e| e.width()))?;
    let image = entry.decode().ok()?;
    Some(resize(
        image.rgba_data(),
        image.width(),
        image.height(),
        size,
    ))
}

/// Area-average resize with premultiplied alpha.
fn resize(src: &[u8], sw: u32, sh: u32, size: u32) -> Vec<u8> {
    if sw == size && sh == size {
        return src.to_vec();
    }
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (x0, x1) = (x * sw / size, ((x + 1) * sw / size).max(x * sw / size + 1));
            let (y0, y1) = (y * sh / size, ((y + 1) * sh / size).max(y * sh / size + 1));
            let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for sy in y0..y1.min(sh) {
                for sx in x0..x1.min(sw) {
                    let i = ((sy * sw + sx) * 4) as usize;
                    let alpha = src[i + 3] as u32;
                    r += src[i] as u32 * alpha;
                    g += src[i + 1] as u32 * alpha;
                    b += src[i + 2] as u32 * alpha;
                    a += alpha;
                    n += 1;
                }
            }
            let o = ((y * size + x) * 4) as usize;
            if let (Some(r), Some(g), Some(b)) =
                (r.checked_div(a), g.checked_div(a), b.checked_div(a))
            {
                out[o] = r as u8;
                out[o + 1] = g as u8;
                out[o + 2] = b as u8;
            }
            out[o + 3] = (a / n.max(1)) as u8;
        }
    }
    out
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let file = BufWriter::new(File::create(path).expect("create png"));
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("png header")
        .write_image_data(rgba)
        .expect("png data");
}
