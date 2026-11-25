#[path = "previews/vtex.rs"]
mod vtex;
use crate::packages;
use image::{DynamicImage, ImageFormat, ImageReader};
use std::{
    fs::{self, File},
    io::{BufWriter, Cursor, Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 24 * 1024 * 1024;
const MAX_ATTEMPTS: usize = 5;

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}
fn raster(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "tga"
    )
}

fn decode_raster(bytes: &[u8], format: Option<ImageFormat>) -> Result<DynamicImage, String> {
    let format = format
        .or_else(|| image::guess_format(bytes).ok())
        .ok_or("Неизвестный формат изображения.")?;
    let open = || ImageReader::with_format(Cursor::new(bytes), format);
    let (w, h) = open().into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || w > 8192 || h > 8192 || u64::from(w) * u64::from(h) > 24_000_000 {
        return Err(
            "Для обложки выберите изображение до 24 Мп и до 8192 пикселей по стороне.".into(),
        );
    }
    let mut decoder = open();
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    decoder.limits(limits);
    decoder.decode().map_err(|e| e.to_string())
}
fn read_image(path: &Path) -> Result<DynamicImage, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    if len == 0 || len > MAX_BYTES as u64 {
        return Err("Выберите непустое изображение до 24 МиБ.".into());
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Изображение превышает 24 МиБ.".into());
    }
    if extension(path) == "vtex_c" {
        vtex::decode(&bytes)
            .ok_or_else(|| "Формат этой текстуры не поддерживает предпросмотр.".into())
    } else {
        decode_raster(&bytes, ImageFormat::from_path(path).ok())
    }
}

fn save(img: DynamicImage, dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let img = if img.width() > 768 || img.height() > 512 {
        img.thumbnail(768, 512)
    } else {
        img
    };
    let path = dir.join(format!("cover-{}.png", crate::store::uid()));
    let file = File::create_new(&path).map_err(|e| e.to_string())?;
    let write = (|| {
        let mut writer = BufWriter::new(file);
        img.write_to(&mut writer, ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
    })();
    if let Err(e) = write {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

pub fn attach(image_path: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    if !raster(image_path) {
        return Err("Выберите PNG, JPEG, TGA или WebP.".into());
    }
    save(read_image(image_path)?, dest_dir)
}

fn sidecar(source: &Path) -> Option<DynamicImage> {
    let stem = source.file_stem()?.to_str()?;
    for suffix in ["", ".preview", "_preview", "-preview"] {
        for ext in ["png", "jpg", "jpeg", "webp"] {
            let p = source.with_file_name(format!("{stem}{suffix}.{ext}"));
            if p != source && p.is_file() {
                if let Ok(img) = read_image(&p) {
                    return Some(img);
                }
            }
        }
    }
    None
}

fn is_preview_name(name: &str) -> bool {
    [
        "screenshot",
        "preview",
        "render",
        "cover",
        "скрин",
        "превью",
        "обложк",
    ]
    .iter()
    .any(|word| name.contains(word))
}
fn zip_picture(source: &Path, asset: &Path) -> Option<DynamicImage> {
    let mut zip = zip::ZipArchive::new(File::open(source).ok()?).ok()?;
    if zip.len() > 8192 {
        return None;
    }
    let ordinal = asset
        .file_name()?
        .to_str()?
        .split_once('-')?
        .0
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    let mut packages = Vec::new();
    let mut images = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).ok()?;
        if !entry.is_file() || entry.is_symlink() {
            continue;
        }
        let name = entry.name().replace('\\', "/").to_lowercase();
        if name
            .split('/')
            .any(|p| p == "__macosx" || p.starts_with("._") || p == "..")
        {
            continue;
        }
        let p = Path::new(&name);
        let parent = p
            .parent()
            .and_then(Path::to_str)
            .unwrap_or_default()
            .to_owned();
        if extension(p) == "vpk" {
            packages.push(parent);
            continue;
        }
        if raster(p) && (1..=MAX_BYTES as u64).contains(&entry.size()) {
            images.push((i, name.clone(), parent, entry.size()));
        }
    }
    let folder = packages.get(ordinal)?;
    let mut ranked = Vec::new();
    for (index, name, parent, size) in images {
        let preview = is_preview_name(&name);
        if packages.len() > 1 && parent != *folder && !parent.is_empty() {
            continue;
        }
        if !preview && (name.contains("materials/") || name.contains("textures/")) {
            continue;
        }
        let rank = if parent == *folder { 0 } else { 2 };
        ranked.push((rank + usize::from(!preview), index, name, size));
    }
    ranked.sort();
    for (_, idx, name, size) in ranked.into_iter().take(MAX_ATTEMPTS) {
        let Ok(entry) = zip.by_index(idx) else {
            continue;
        };
        let mut bytes = Vec::with_capacity(size as usize);
        if entry.take(size + 1).read_to_end(&mut bytes).is_err() || bytes.len() != size as usize {
            continue;
        }
        if let Ok(img) = decode_raster(&bytes, ImageFormat::from_path(name).ok()) {
            return Some(img);
        }
    }
    None
}

fn rank_resource(path: &str) -> Option<(u8, &'static str)> {
    let native = path.ends_with(".vtex_c");
    if !native && !raster(Path::new(path)) {
        return None;
    }
    if is_preview_name(path) {
        return Some((0, "Изображение из пакета"));
    }
    if path.starts_with("panorama/images/") {
        let rank = if path.contains("/heroes/") || path.contains("/heroes_horizontal/") {
            1
        } else if path.contains("/econ/") || path.contains("/items/") {
            2
        } else {
            4
        };
        return Some((rank, "Игровая иконка из пакета"));
    }
    if native && (path.contains("_color") || path.contains("albedo") || path.contains("diffuse")) {
        return Some((5, "Цветовая текстура из пакета"));
    }
    if !native {
        Some((3, "Изображение из пакета"))
    } else {
        None
    }
}
fn vpk_picture(asset: &Path) -> Option<(DynamicImage, String)> {
    let names = packages::inspect(asset).ok()?;
    let mut candidates: Vec<_> = names
        .iter()
        .filter_map(|p| {
            let (rank, caption) = rank_resource(p)?;
            Some((rank, p, caption))
        })
        .collect();
    candidates.sort_unstable();
    for (_, path, caption) in candidates.into_iter().take(MAX_ATTEMPTS) {
        let Ok(bytes) = packages::read_member(asset, path, MAX_BYTES) else {
            continue;
        };
        let img = if path.ends_with(".vtex_c") {
            vtex::decode(&bytes)
        } else {
            decode_raster(&bytes, ImageFormat::from_path(path).ok()).ok()
        };
        if let Some(img) = img {
            return Some((img, caption.into()));
        }
    }
    None
}

/// Returns a managed, display-sized PNG and a truthful description of its source.
pub fn discover(
    source: &Path,
    asset: &Path,
    dest_dir: &Path,
) -> Result<Option<(PathBuf, String)>, String> {
    let mut found = if raster(source) {
        read_image(source)
            .ok()
            .map(|image| (image, String::from("Исходная текстура")))
    } else {
        None
    };
    if extension(source) == "zip" {
        found = zip_picture(source, asset).map(|im| (im, String::from("Изображение из ZIP")));
    }
    if found.is_none() && !raster(source) {
        found = sidecar(source).map(|im| (im, String::from("Изображение рядом с модом")));
    }
    if found.is_none() {
        found = match extension(asset).as_str() {
            "vpk" => vpk_picture(asset),
            "vtex_c" => read_image(asset)
                .ok()
                .map(|im| (im, String::from("Текстура ресурса"))),
            _ => None,
        };
    }
    if let Some((img, caption)) = found {
        Ok(Some((save(img, dest_dir)?, caption)))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
#[path = "previews/tests.rs"]
mod tests;
