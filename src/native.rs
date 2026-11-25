use std::{
    ffi::{OsStr, OsString},
    mem::{size_of, zeroed},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::*,
    Media::Audio::*,
    System::{Registry::*, Threading::CreateMutexW},
    UI::{Controls::Dialogs::*, WindowsAndMessaging::*},
};

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
pub fn message(text: &str) {
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Rift Atelier").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
pub struct Instance(HANDLE);
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn instance() -> Option<Instance> {
    let mut name = String::from("Local\\RiftAtelier-18e85c26");
    let key = |path: PathBuf| {
        let path = path
            .canonicalize()
            .or_else(|_| std::path::absolute(&path))
            .unwrap_or(path);
        let normalized = path
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase();
        if let Some(unc) = normalized.strip_prefix(r"\\?\unc\") {
            format!(r"\\{unc}")
        } else {
            normalized
                .strip_prefix(r"\\?\")
                .unwrap_or(&normalized)
                .to_string()
        }
    };
    let standard =
        std::env::var_os("LOCALAPPDATA").map(|p| key(PathBuf::from(p).join("RiftAtelier")));
    if let Some(path) = std::env::var_os("RIFT_ATELIER_DATA").map(|p| key(PathBuf::from(p)))
        && standard.as_ref() != Some(&path)
    {
        use std::hash::{Hash, Hasher};
        let mut hash = std::hash::DefaultHasher::new();
        path.hash(&mut hash);
        name.push_str(&format!("-{:016x}", hash.finish()));
    }
    unsafe {
        let h = CreateMutexW(ptr::null(), 0, wide(&name).as_ptr());
        if h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
            if !h.is_null() {
                CloseHandle(h);
            }
            return None;
        }
        Some(Instance(h))
    }
}
pub const WINDOW_TITLE: &str = concat!(
    "Rift Atelier ",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR"),
    " — Мастерская модов Dota 2"
);

/// On upgrade the old release may still own the shared library lock.
pub fn launch_instance() -> Option<Instance> {
    if let Some(lock) = instance() {
        return Some(lock);
    }
    let old = [
        "Rift Atelier 1.2 — Мастерская модов Dota 2",
        "Rift Atelier 1.1 — Мастерская модов Dota 2",
        "Rift Atelier — Мастерская модов Dota 2",
    ]
    .iter()
    .map(|title| unsafe { FindWindowW(ptr::null(), wide(title).as_ptr()) })
    .find(|window| !window.is_null());
    if let Some(old) = old {
        let question = wide(
            "Сейчас открыта предыдущая версия Rift Atelier. Закрыть её и открыть новую версию с той же библиотекой?",
        );
        let answer = unsafe {
            MessageBoxW(
                ptr::null_mut(),
                question.as_ptr(),
                wide("Обновление Rift Atelier").as_ptr(),
                MB_YESNO | MB_ICONQUESTION,
            )
        };
        if answer != IDYES {
            return None;
        }
        unsafe {
            PostMessageW(old, WM_CLOSE, 0, 0);
        }
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if let Some(lock) = instance() {
                return Some(lock);
            }
        }
        message(
            "Завершите текущую операцию в старом окне, закройте его и снова запустите Rift Atelier.",
        );
    } else {
        let window = unsafe { FindWindowW(ptr::null(), wide(WINDOW_TITLE).as_ptr()) };
        if !window.is_null() {
            unsafe {
                ShowWindow(window, SW_RESTORE);
                SetForegroundWindow(window);
            }
        } else {
            message("Rift Atelier уже открыт.");
        }
    }
    None
}
pub fn steam_path() -> Option<PathBuf> {
    let mut buf = [0u16; 8192];
    let mut len = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide("Software\\Valve\\Steam").as_ptr(),
            wide("SteamPath").as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    (status == 0).then(|| {
        PathBuf::from(OsString::from_wide(
            &buf[..buf.iter().position(|x| *x == 0).unwrap_or(buf.len())],
        ))
    })
}
fn decode_selection(buf: &[u16]) -> Result<Vec<PathBuf>, String> {
    let mut parts = buf
        .split(|&c| c == 0)
        .take_while(|p| !p.is_empty())
        .map(|part| PathBuf::from(OsString::from_wide(part)));
    let first = parts.next().ok_or("Windows вернула пустой путь.")?;
    let mut paths = parts.map(|name| first.join(name)).collect::<Vec<_>>();
    if paths.is_empty() {
        paths.push(first);
    }
    Ok(paths)
}

fn dialog_failure(code: COMMON_DLG_ERRORS) -> Result<Vec<PathBuf>, String> {
    match code {
        0 => Ok(Vec::new()),
        FNERR_BUFFERTOOSMALL => {
            Err("Выбрано слишком много файлов. Выберите меньше файлов за один раз.".into())
        }
        FNERR_INVALIDFILENAME => {
            Err("Windows не распознала имя файла. Проверьте путь и попробуйте ещё раз.".into())
        }
        other => Err(format!(
            "Windows не смогла открыть окно выбора файла (0x{other:04X}). Можно перетащить файл из Проводника в Rift Atelier."
        )),
    }
}

fn file_dialog(
    pattern: &str,
    title: &str,
    save: bool,
    multiple: bool,
) -> Result<Vec<PathBuf>, String> {
    let mut buf = vec![0u16; 65536];
    let filter = wide(format!(
        "{title} ({pattern})\0{pattern}\0Все файлы (*.*)\0*.*\0"
    ));
    let cap = wide(title);
    let extension = wide("vpk");
    unsafe {
        let mut dialog: OPENFILENAMEW = zeroed();
        dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        dialog.hwndOwner = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow();
        dialog.lpstrFilter = filter.as_ptr();
        dialog.nFilterIndex = 1;
        dialog.lpstrFile = buf.as_mut_ptr();
        dialog.nMaxFile = buf.len() as u32;
        dialog.lpstrTitle = cap.as_ptr();
        dialog.Flags = OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save { 0 } else { OFN_FILEMUSTEXIST };
        if multiple {
            dialog.Flags |= OFN_ALLOWMULTISELECT;
        }
        if save {
            dialog.lpstrDefExt = extension.as_ptr();
        }
        let ok = if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        };
        if ok == 0 {
            return dialog_failure(CommDlgExtendedError());
        }
    }
    decode_selection(&buf)
}

pub fn pick_many(pattern: &str, title: &str) -> Result<Vec<PathBuf>, String> {
    file_dialog(pattern, title, false, true)
}

pub fn choose(pattern: &str, title: &str, save: bool) -> Option<PathBuf> {
    match file_dialog(pattern, title, save, false) {
        Ok(mut paths) => paths.pop(),
        Err(error) => {
            message(&error);
            None
        }
    }
}
pub fn sound(path: Option<&Path>) -> Result<(), String> {
    let buffer = path.map(wide);
    let p = buffer.as_ref().map_or(ptr::null(), |s| s.as_ptr());
    let ok = unsafe { PlaySoundW(p, ptr::null_mut(), SND_ASYNC | SND_FILENAME | SND_NODEFAULT) };
    if ok == 0 && path.is_some() {
        Err("Windows не смогла воспроизвести WAV.".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_single_and_multiple_paths_from_windows_dialog() {
        let single = wide("C:\\Моды\\Новый герой.zip\0");
        assert_eq!(
            decode_selection(&single).unwrap(),
            [PathBuf::from("C:\\Моды\\Новый герой.zip")]
        );
        let multiple = wide("D:\\Мои файлы\0Текстура.png\0Модель героя.vpk\0");
        assert_eq!(
            decode_selection(&multiple).unwrap(),
            [
                PathBuf::from("D:\\Мои файлы\\Текстура.png"),
                PathBuf::from("D:\\Мои файлы\\Модель героя.vpk")
            ]
        );
    }

    #[test]
    fn cancellation_and_windows_failure_are_different_results() {
        assert!(dialog_failure(0).unwrap().is_empty());
        assert!(
            dialog_failure(FNERR_BUFFERTOOSMALL)
                .unwrap_err()
                .contains("меньше")
        );
        assert!(dialog_failure(CDERR_INITIALIZATION).is_err());
    }
}
