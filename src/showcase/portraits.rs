use image::{DynamicImage, ImageFormat, ImageReader};
use std::io::Cursor;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs};

const MAX_BYTES: u64 = 4 * 1024 * 1024;

fn decode(bytes: &[u8]) -> Option<DynamicImage> {
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
        .into_dimensions()
        .ok()?;
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
        return None;
    }
    image::load_from_memory_with_format(bytes, ImageFormat::Png).ok()
}

pub(super) fn load(cache: &Path, hero: &str) -> Option<DynamicImage> {
    let file = cache.join(format!("{hero}.png"));
    if fs::metadata(&file).is_ok_and(|m| m.len() <= MAX_BYTES)
        && let Some(image) = fs::read(&file).ok().and_then(|b| decode(&b))
    {
        return Some(image.thumbnail(600, 600));
    }
    let url = format!(
        "https://cdn.fastly.steamstatic.com/apps/dota2/videos/dota_react/heroes/renders/{hero}.png"
    );
    let curl = PathBuf::from(env::var_os("SystemRoot")?).join("System32/curl.exe");
    let result = Command::new(curl)
        .creation_flags(0x0800_0000)
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--proto",
            "=https",
            "--connect-timeout",
            "5",
            "--max-time",
            "18",
            "--max-filesize",
            "4194304",
            "--url",
            &url,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !result.status.success() || result.stdout.len() as u64 > MAX_BYTES {
        return None;
    }
    let image = decode(&result.stdout)?.thumbnail(600, 600);
    if fs::create_dir_all(cache).is_ok() {
        let pending = file.with_extension("part");
        if image.save_with_format(&pending, ImageFormat::Png).is_ok() {
            let _ = fs::rename(&pending, &file);
        }
        let _ = fs::remove_file(pending);
    }
    Some(image)
}
