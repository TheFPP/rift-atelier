use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SITE: &str = "https://h6rd.github.io/Dota2PornFxWeb/";

// Links use the same category parameter as the website's Share button.
pub const SECTIONS: &[(&str, &str)] = &[
    (
        "Герои",
        "https://h6rd.github.io/Dota2PornFxWeb/?category=heroes",
    ),
    (
        "Предметы героев",
        "https://h6rd.github.io/Dota2PornFxWeb/?category=hero-items",
    ),
    (
        "Эффекты героев",
        "https://h6rd.github.io/Dota2PornFxWeb/?category=herofx",
    ),
    (
        "Звуки",
        "https://h6rd.github.io/Dota2PornFxWeb/?category=sounds",
    ),
    (
        "Озвучка героев",
        "https://h6rd.github.io/Dota2PornFxWeb/?category=hero-sounds",
    ),
];

#[cfg(windows)]
pub fn downloads_dir() -> Option<PathBuf> {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath};
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(ptr: *mut c_void);
    }
    let mut ptr = std::ptr::null_mut();
    // The Shell returns an allocated, NUL-terminated UTF-16 folder path.
    let hr =
        unsafe { SHGetKnownFolderPath(&FOLDERID_Downloads, 0, std::ptr::null_mut(), &mut ptr) };
    let folder = if hr >= 0 && !ptr.is_null() {
        unsafe {
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            Some(PathBuf::from(OsString::from_wide(
                std::slice::from_raw_parts(ptr, len),
            )))
        }
    } else {
        None
    };
    unsafe {
        CoTaskMemFree(ptr.cast());
    }
    folder.or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads")))
}

#[cfg(not(windows))]
pub fn downloads_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Downloads"))
}

pub fn recent_downloads(dir: &Path, limit: usize) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in
        fs::read_dir(dir).map_err(|e| format!("Не удалось открыть {}. {e}", dir.display()))?
    {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !ext.eq_ignore_ascii_case("vpk") && !ext.eq_ignore_ascii_case("zip") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_file() && !meta.is_symlink() && meta.len() > 0 {
            files.push((meta.modified().unwrap_or(UNIX_EPOCH), path));
        }
    }
    files.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(files
        .into_iter()
        .filter(|(_, path)| validate_package(path).is_ok())
        .take(limit)
        .map(|(_, path)| path)
        .collect())
}

const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 8192;
const MAX_VPK_FILES: usize = 256;

fn file_size(path: &Path, file: &File) -> Result<u64, String> {
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        return Err(format!(
            "«{name}» — пустой файл (0 Б). Скачайте мод ещё раз и дождитесь завершения загрузки."
        ));
    }
    Ok(size)
}

pub fn check_source(path: &Path) -> Result<(), String> {
    let file =
        File::open(path).map_err(|e| format!("Не удалось открыть {}. {e}", path.display()))?;
    file_size(path, &file).map(|_| ())
}

/// Check a complete browser download before publishing it or offering to import it.
/// Recognizes temporary names and reads only the package headers and ZIP directory.
pub fn validate_package(path: &Path) -> Result<(), String> {
    let mut file =
        File::open(path).map_err(|e| format!("Не удалось открыть {}. {e}", path.display()))?;
    let size = file_size(path, &file)?;
    let mut header = [0; 12];
    let invalid_format = "Сервер вернул файл другого формата или загрузка не завершилась. Нужен VPK либо ZIP с модом.";
    file.read_exact(&mut header).map_err(|_| invalid_format)?;
    if valid_vpk_header(&header, size) {
        return Ok(());
    }
    if !header.starts_with(b"PK") {
        return Err(invalid_format.into());
    }
    let mut archive = open_zip(path)?;
    let mut count = 0usize;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let raw = archive.by_index_raw(index).map_err(|e| e.to_string())?;
        if !is_vpk_name(raw.name()) {
            continue;
        }
        let size = raw.size();
        if size > MAX_ARCHIVE_BYTES - total || count == MAX_VPK_FILES {
            return Err("Один импорт поддерживает до 256 VPK общим размером до 1 ГиБ.".into());
        }
        check_member(&raw)?;
        drop(raw);
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("Не удалось открыть VPK внутри ZIP. {e}"))?;
        if entry.read_exact(&mut header).is_err() || !valid_vpk_header(&header, size) {
            return Err(format!(
                "Файл «{}» в ZIP повреждён или является отдельным томом VPK.",
                entry.name()
            ));
        }
        count += 1;
        total += size;
    }
    if count == 0 {
        return Err("В этом ZIP нет файлов VPK.".into());
    }
    Ok(())
}

fn is_vpk_name(name: &str) -> bool {
    let path = Path::new(name);
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("vpk"))
        && !name
            .split(['/', '\\'])
            .any(|part| part == "__MACOSX" || part.starts_with("._"))
}

fn check_member<R: Read>(raw: &zip::read::ZipFile<'_, R>) -> Result<(), String> {
    let name = raw.name();
    if raw.is_symlink()
        || !raw.is_file()
        || raw.enclosed_name().is_none()
        || name.starts_with(['/', '\\'])
        || name.contains(':')
        || name.split(['/', '\\']).any(|part| part == "..")
    {
        Err("Некорректный путь к VPK внутри ZIP.".into())
    } else {
        Ok(())
    }
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<File>, String> {
    let input = File::open(path).map_err(|e| format!("Не удалось открыть ZIP. {e}"))?;
    if file_size(path, &input)? > MAX_ARCHIVE_BYTES {
        return Err("Размер ZIP превышает 1 ГиБ.".into());
    }
    let archive = zip::ZipArchive::new(input).map_err(|_| {
        "ZIP не удалось прочитать. Если файл ещё загружается, дождитесь завершения. Если загрузка завершена, скачайте архив повторно.".to_owned()
    })?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err("В ZIP больше 8192 файлов.".into());
    }
    Ok(archive)
}

struct ImportDir(Option<PathBuf>);

impl ImportDir {
    fn create(cache: &Path) -> io::Result<Self> {
        fs::create_dir_all(cache)?;
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        loop {
            let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = cache.join(format!("zip-{}-{stamp}-{seq}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(Some(path))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for ImportDir {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn safe_file_name(index: usize, path: &Path) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .take(55)
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let stem = clean(path.file_stem().and_then(|s| s.to_str()).unwrap_or("mod"));
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());
    match parent {
        Some(label) => format!("{index:03}-{}-{stem}.vpk", clean(label)),
        None => format!("{index:03}-{stem}.vpk"),
    }
}

fn valid_vpk_header(header: &[u8; 12], size: u64) -> bool {
    let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
    let tree = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
    let header_len = match version {
        1 => 12,
        2 => 28,
        _ => return false,
    };
    header[..4] == [0x34, 0x12, 0xaa, 0x55] && tree > 0 && header_len + tree <= size
}

fn copy_vpk(input: &mut impl Read, size: u64, path: &Path) -> Result<(), String> {
    let mut header = [0; 12];
    let fail =
        || "Архив содержит повреждённый VPK или отдельный том многотомного пакета.".to_string();
    input.read_exact(&mut header).map_err(|_| fail())?;
    if !valid_vpk_header(&header, size) {
        return Err(fail());
    }
    let mut output = File::create_new(path).map_err(|e| e.to_string())?;
    output.write_all(&header).map_err(|e| e.to_string())?;
    // One extra byte forces the decoder's EOF/CRC check and catches a forged size.
    let written = io::copy(&mut input.take(size - 12 + 1), &mut output)
        .map_err(|e| format!("Ошибка распаковки VPK. {e}"))?;
    if written + 12 != size {
        return Err(fail());
    }
    output.sync_all().map_err(|e| e.to_string())
}

/// Imports complete, embedded VPKs from a ZIP. The returned files live under
/// an exclusively created cache directory. An unsuccessful import rolls back.
pub fn extract_vpks(zip_path: &Path, cache_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut archive = open_zip(zip_path)?;
    let mut dir = ImportDir::create(cache_dir)
        .map_err(|e| format!("Не удалось создать папку импорта. {e}"))?;
    let dest = dir.0.as_ref().unwrap();
    let mut paths = Vec::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        // Inspect metadata first, so unsupported compression on an unrelated
        // readme, installer, or preview does not prevent importing the VPK.
        let raw = archive.by_index_raw(i).map_err(|e| e.to_string())?;
        let normalized = raw.name().replace('\\', "/");
        let name = Path::new(&normalized);
        if !is_vpk_name(&normalized) {
            continue;
        }
        check_member(&raw)?;
        let size = raw.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| "Неверный размер файлов в ZIP.".to_string())?;
        if total > MAX_ARCHIVE_BYTES || paths.len() >= MAX_VPK_FILES {
            return Err("Один импорт поддерживает до 256 VPK общим размером до 1 ГиБ.".into());
        }
        let path = dest.join(safe_file_name(paths.len() + 1, name));
        drop(raw);
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Не удалось открыть VPK внутри ZIP. {e}"))?;
        copy_vpk(&mut entry, size, &path)?;
        paths.push(path);
    }
    if paths.is_empty() {
        return Err("В этом ZIP нет файлов VPK.".into());
    }
    dir.0.take();
    Ok(paths)
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
