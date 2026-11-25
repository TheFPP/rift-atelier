use crate::{
    library,
    store::{Item, uid},
};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use url::Url;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Default)]
pub struct Progress {
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
    pub importing: AtomicBool,
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn address(text: &str) -> Result<Url, String> {
    let url = Url::parse(text.trim())
        .map_err(|_| "Вставьте полную HTTPS-ссылку на файл ZIP или VPK.".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Нужна HTTPS-ссылка на скачивание без логина и пароля в адресе.".into());
    }
    Ok(url)
}
fn filename(url: &Url, ext: &str) -> String {
    let name = url
        .path_segments()
        .and_then(|mut path| path.next_back())
        .unwrap_or("");
    let decoded = percent_encoding::percent_decode_str(name).decode_utf8_lossy();
    let mut safe = String::new();
    for c in decoded.chars().take(130) {
        if c.is_alphanumeric() || " -_.".contains(c) {
            safe.push(c)
        } else {
            safe.push('_')
        }
    }
    let clean = safe.trim_matches(['.', ' ']);
    let stem = Path::new(clean)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Загрузка");
    let reserved = stem.split('.').next().unwrap_or("").to_uppercase();
    let device = matches!(reserved.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (["COM", "LPT"].iter().any(|s| {
            reserved
                .strip_prefix(s)
                .is_some_and(|digit| digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit())
        }));
    format!("{}{stem}.{ext}", if device { "_" } else { "" })
}
fn package_extension(header: &[u8]) -> Result<&'static str, String> {
    if header.starts_with(b"PK\x03\x04") {
        Ok("zip")
    } else if header.starts_with(&[0x34, 0x12, 0xaa, 0x55]) {
        Ok("vpk")
    } else if header.is_empty() {
        Err("Сервер прислал 0 Б. Попробуйте прямую ссылку кнопки скачивания.".into())
    } else {
        Err("По этой ссылке пришла страница или неподдерживаемый файл. Скопируйте адрес самого ZIP или VPK.".into())
    }
}
fn copy_download(
    input: &mut impl Read,
    output: &mut File,
    progress: &Progress,
) -> Result<&'static str, String> {
    let mut head = [0; 4];
    let mut read = 0;
    while read < head.len() {
        let n = input.read(&mut head[read..]).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        read += n;
    }
    let ext = package_extension(&head[..read])?;
    output.write_all(&head).map_err(|e| e.to_string())?;
    let mut total = 4u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("Загрузка отменена.".into());
        }
        let n = input.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_BYTES {
            return Err("Размер файла превышает 1 ГиБ.".into());
        }
        output.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        progress.bytes.store(total, Ordering::Relaxed);
    }
    output.sync_all().map_err(|e| e.to_string())?;
    Ok(ext)
}
/// The system HTTPS client inherits Windows proxy and certificate settings.
pub fn import_url(
    data: &Path,
    root: &Path,
    text: &str,
    progress: &Progress,
) -> Result<Vec<Item>, String> {
    let url = address(text)?;
    let folder = data.join("incoming").join(uid());
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let _scratch = Scratch(folder.clone());
    let temp = folder.join("download.part");
    let mut out = File::create(&temp).map_err(|e| e.to_string())?;
    let diag = folder.join("transport.log");
    let errors = File::create(&diag).map_err(|e| e.to_string())?;
    let curl = PathBuf::from(
        std::env::var_os("SystemRoot").ok_or("Не найден системный каталог Windows.")?,
    )
    .join("System32/curl.exe");
    let mut child = Command::new(curl)
        .creation_flags(0x0800_0000)
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-redirs",
            "8",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--speed-limit",
            "1",
            "--speed-time",
            "20",
            "--max-time",
            "600",
            "--globoff",
            "--user-agent",
            concat!("RiftAtelier/", env!("CARGO_PKG_VERSION")),
            "--url",
        ])
        .arg(url.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(errors))
        .spawn()
        .map_err(|e| format!("Не удалось открыть системный HTTPS-клиент: {e}"))?;
    let mut input = child.stdout.take().ok_or("Нет потока загрузки.")?;
    let result = std::thread::scope(|scope| {
        use std::sync::mpsc::{RecvTimeoutError, sync_channel};
        let (tx, rx) = sync_channel(1);
        scope.spawn(move || {
            let _ = tx.send(copy_download(&mut input, &mut out, progress));
        });
        loop {
            if progress.cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                break Err("Загрузка отменена.".into());
            }
            match rx.recv_timeout(std::time::Duration::from_millis(90)) {
                Ok(result) => break result,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    break Err("Чтение ответа сервера прервалось.".into());
                }
            }
        }
    });
    if result.is_err() && child.try_wait().map_err(|e| e.to_string())?.is_none() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if progress.cancel.load(Ordering::Relaxed) {
        return Err("Загрузка отменена.".into());
    }
    if !status.success() {
        let detail = fs::read_to_string(diag).unwrap_or_default();
        if !detail.trim().is_empty() {
            return Err(format!(
                "Не удалось скачать файл. {}",
                detail.trim().chars().take(320).collect::<String>()
            ));
        }
        if result.is_ok() {
            return Err("Передача прервалась до завершения. Попробуйте ещё раз.".into());
        }
    }
    let ext = result?;
    progress.importing.store(true, Ordering::SeqCst);
    if progress.cancel.load(Ordering::SeqCst) {
        return Err("Загрузка отменена.".into());
    }
    let dest = folder.join(filename(&url, ext));
    fs::rename(temp, &dest).map_err(|e| e.to_string())?;
    library::import(data, root, &dest, "")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_empty_webpages_and_unsafe_schemes() {
        assert!(package_extension(b"").unwrap_err().contains("0 Б"));
        assert!(package_extension(b"<!DOCTYPE html>").is_err());
        assert_eq!(package_extension(b"PK\x03\x04").unwrap(), "zip");
        assert!(address("file:///C:/data.zip").is_err());
        assert!(address("https://user:pass@example.com/mod.zip").is_err());
    }
    #[test]
    fn handles_download_names_and_windows_devices() {
        assert_eq!(
            filename(
                &address("https://example.org/Bare.Brewmaster.zip?x=y").unwrap(),
                "zip"
            ),
            "Bare.Brewmaster.zip"
        );
        assert_eq!(
            filename(&address("https://example.org/NUL.zip").unwrap(), "zip"),
            "_NUL.zip"
        );
    }
}
