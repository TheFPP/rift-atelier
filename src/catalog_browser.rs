//! A top-level, first-party browsing session in an on-demand Windows child WebView.
//! The website owns its search, galleries, file selection and download links.
use eframe::egui::{self, Rect};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::OsStr,
    fs,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender},
};
use wry::{
    WebContext, WebView, WebViewBuilder,
    dpi::{PhysicalPosition, PhysicalSize},
    raw_window_handle::HasWindowHandle,
};

pub enum BrowserEvent {
    Downloaded(PathBuf),
    Error(String),
}

struct Stage {
    dir: PathBuf,
    file: PathBuf,
    url: String,
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.file);
        let _ = fs::remove_dir(&self.dir);
    }
}

fn is_package(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("zip") || s.eq_ignore_ascii_case("vpk"))
}

fn stage_download(downloads: &Path, suggested: &Path, url: &str) -> Result<Stage, String> {
    let name = suggested
        .file_name()
        .ok_or("У загрузки отсутствует имя файла.")?;
    if !is_package(Path::new(name)) || name.to_string_lossy().contains(':') {
        return Err("Для импорта выберите VPK или ZIP.".into());
    }
    if !downloads.is_absolute() {
        return Err("Не удалось определить папку загрузок.".into());
    }
    fs::create_dir_all(downloads).map_err(|e| e.to_string())?;
    let dir = downloads.join(format!(".rift-transfer-{}", crate::store::uid()));
    fs::create_dir(&dir).map_err(|e| format!("Не удалось сохранить файл в «Загрузки»: {e}"))?;
    Ok(Stage {
        file: dir.join(name),
        dir,
        url: url.to_owned(),
    })
}

// The staging directory is on the destination volume. MoveFileW publishes the complete
// archive atomically and refuses to replace an existing file, even if another process races.
fn publish(stage: Stage, downloads: &Path) -> Result<PathBuf, String> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileW;
    crate::catalog::validate_package(&stage.file)?;
    let stem = stage.file.file_stem().unwrap_or_default().to_string_lossy();
    let ext = stage.file.extension().unwrap_or_default().to_string_lossy();
    let from: Vec<u16> = stage
        .file
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    for n in 0..10000 {
        let name = if n == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let dest = downloads.join(name);
        let to: Vec<u16> = dest.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileW(from.as_ptr(), to.as_ptr()) } != 0 {
            return Ok(dest);
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(format!("Не удалось сохранить загрузку: {e}"));
        }
    }
    Err("В папке слишком много файлов с таким названием.".into())
}

#[derive(Default)]
struct Transfers {
    stages: HashMap<PathBuf, Stage>,
    interrupted: HashMap<String, usize>,
}

impl Transfers {
    fn start(&mut self, dir: &Path, path: &mut PathBuf, url: &str) -> Result<(), String> {
        let stage = stage_download(dir, path, url)?;
        *path = stage.file.clone();
        self.stages.insert(path.clone(), stage);
        Ok(())
    }

    fn finish(
        &mut self,
        dir: &Path,
        url: &str,
        path: Option<PathBuf>,
        ok: bool,
    ) -> Option<BrowserEvent> {
        let event = if ok {
            let stage = path.and_then(|p| self.stages.remove(&p))?;
            match publish(stage, dir) {
                Ok(path) => BrowserEvent::Downloaded(path),
                Err(error) => BrowserEvent::Error(error),
            }
        } else {
            if !self.stages.values().any(|s| s.url == url) {
                return None;
            }
            *self.interrupted.entry(url.to_owned()).or_default() += 1;
            BrowserEvent::Error("Загрузка прервана. Выберите файл ещё раз.".into())
        };
        // An interrupted WebView2 download has no reported path. Concurrent requests for
        // the same URL may still succeed; leave their files alone until the group settles.
        let remaining = self.stages.values().filter(|s| s.url == url).count();
        if self.interrupted.get(url).is_some_and(|n| *n >= remaining) {
            self.stages.retain(|_, s| s.url != url);
            self.interrupted.remove(url);
        }
        Some(event)
    }
}
pub struct Browser {
    view: Option<WebView>,
    events: (Sender<BrowserEvent>, Receiver<BrowserEvent>),
    selected_url: String,
    next_url: Rc<RefCell<Option<String>>>,
    pub local_files: bool,
    target: Option<Rect>,
    suspended: bool,
    last_bounds: Option<wry::Rect>,
    active: Rc<RefCell<Transfers>>,
    ready: Rc<Cell<bool>>,
    failed: bool,
}
impl Default for Browser {
    fn default() -> Self {
        Self {
            view: None,
            events: mpsc::channel(),
            selected_url: crate::catalog::SECTIONS[0].1.to_owned(),
            next_url: Rc::default(),
            local_files: false,
            target: None,
            suspended: false,
            last_bounds: None,
            active: Rc::default(),
            ready: Rc::default(),
            failed: false,
        }
    }
}
impl Browser {
    pub fn begin_frame(&mut self) {
        self.target = None;
        self.suspended = false;
    }
    pub fn hide_for_frame(&mut self) {
        self.suspended = self.target.is_some();
        self.target = None;
    }
    pub fn show_at(&mut self, rect: Rect) {
        self.target = Some(rect);
    }
    pub fn take_events(&self) -> Vec<BrowserEvent> {
        self.events.1.try_iter().collect()
    }
    pub fn loading(&self) -> bool {
        !self.ready.get() && !self.failed
    }
    pub fn download_count(&self) -> usize {
        self.active.borrow().stages.len()
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn navigate(&mut self, url: &str) {
        self.selected_url = url.to_owned();
        self.local_files = false;
        self.failed = false;
        self.ready.set(false);
        *self.next_url.borrow_mut() = Some(url.to_owned());
    }
    pub fn reload(&mut self) {
        self.failed = false;
        self.ready.set(false);
        if let Some(v) = &self.view {
            let _ = v.reload();
        }
    }
    pub fn sync<W: HasWindowHandle>(&mut self, parent: &W, ctx: &egui::Context, data: &Path) {
        let Some(target) = self.target else {
            if !self.suspended && self.active.borrow().stages.is_empty() {
                self.view = None;
                self.last_bounds = None;
                self.ready.set(false);
            } else if self.last_bounds.take().is_some() {
                if let Some(v) = &self.view {
                    let _ = v.set_visible(false);
                }
            }
            return;
        };
        let scale = ctx.pixels_per_point();
        let bounds = wry::Rect {
            position: PhysicalPosition::new(
                (target.left() * scale).round() as i32,
                (target.top() * scale).round() as i32,
            )
            .into(),
            size: PhysicalSize::new(
                (target.width() * scale).round().max(1.) as u32,
                (target.height() * scale).round().max(1.) as u32,
            )
            .into(),
        };
        if self.view.is_none() && !self.failed {
            let client = self.open(parent, ctx, data, bounds);
            match client {
                Ok(view) => {
                    self.view = Some(view);
                    self.last_bounds = Some(bounds);
                    self.next_url.borrow_mut().take();
                }
                Err(e) => {
                    self.failed = true;
                    let _ = self.events.0.send(BrowserEvent::Error(e));
                    ctx.request_repaint();
                }
            }
        } else if let Some(view) = &self.view {
            if self.last_bounds != Some(bounds) {
                let _ = view.set_bounds(bounds);
                let _ = view.set_visible(true);
                self.last_bounds = Some(bounds);
            }
            if let Some(url) = self.next_url.borrow_mut().take() {
                if let Err(e) = view.load_url(&url) {
                    let _ = self.events.0.send(BrowserEvent::Error(e.to_string()));
                    ctx.request_repaint();
                }
            }
        }
    }
    fn open<W: HasWindowHandle>(
        &self,
        parent: &W,
        ctx: &egui::Context,
        data: &Path,
        bounds: wry::Rect,
    ) -> Result<WebView, String> {
        let downloads = crate::catalog::downloads_dir().ok_or("Не найдена папка «Загрузки».")?;
        self.open_in(parent, ctx, data, bounds, &downloads)
    }
    pub(crate) fn open_in<W: HasWindowHandle>(
        &self,
        parent: &W,
        ctx: &egui::Context,
        data: &Path,
        bounds: wry::Rect,
        downloads: &Path,
    ) -> Result<WebView, String> {
        let profile = data.join("webview");
        fs::create_dir_all(&profile).map_err(|e| format!("Не удалось открыть каталог: {e}"))?;
        let mut web_context = WebContext::new(Some(profile));
        let started = self.active.clone();
        let finished = self.active.clone();
        let start_tx = self.events.0.clone();
        let end_tx = self.events.0.clone();
        let start_ui = ctx.clone();
        let end_ui = ctx.clone();
        let page_ui = ctx.clone();
        let popup_ui = ctx.clone();
        let out = downloads.to_path_buf();
        let dest = out.clone();
        let ready = self.ready.clone();
        let next = self.next_url.clone();
        let drop_tx = self.events.0.clone();
        let drop_ui = ctx.clone();
        let on_start = move |url: String, path: &mut PathBuf| {
            let result = started.borrow_mut().start(&out, path, &url);
            start_ui.request_repaint();
            if let Err(error) = result {
                let _ = start_tx.send(BrowserEvent::Error(error));
                return false;
            }
            true
        };
        let on_complete = move |url: String, path: Option<PathBuf>, success: bool| {
            let event = finished.borrow_mut().finish(&dest, &url, path, success);
            if let Some(event) = event {
                let _ = end_tx.send(event);
                end_ui.request_repaint();
            }
        };
        let on_popup = move |url: String, _: wry::NewWindowFeatures| {
            if url.starts_with("https://") {
                *next.borrow_mut() = Some(url);
                popup_ui.request_repaint();
            }
            wry::NewWindowResponse::Deny
        };
        let builder = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_url(&self.selected_url)
            .with_bounds(bounds)
            .with_background_color((24, 24, 27, 255))
            .with_devtools(false)
            .with_permission_handler(|_| wry::PermissionResponse::Deny)
            .with_navigation_handler(|url| {
                url.starts_with("https://")
                    || url.starts_with("blob:https://")
                    || url == "about:blank"
            })
            .with_new_window_req_handler(on_popup)
            .with_drag_drop_handler(move |event| {
                if let wry::DragDropEvent::Drop { paths, .. } = event {
                    for path in paths {
                        let _ = drop_tx.send(BrowserEvent::Downloaded(path));
                    }
                    drop_ui.request_repaint();
                }
                true
            })
            .with_on_page_load_handler(move |event, _| {
                ready.set(matches!(event, wry::PageLoadEvent::Finished));
                page_ui.request_repaint();
            })
            .with_download_started_handler(on_start)
            .with_download_completed_handler(on_complete);
        builder.build_as_child(parent).map_err(|error| {
            format!("Не удалось открыть встроенный каталог. Проверьте установку Microsoft Edge WebView2 Runtime. {error}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VPK: &[u8] = &[0x34, 0x12, 0xaa, 0x55, 1, 0, 0, 0, 1, 0, 0, 0, 0];

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("rift-browser-test-{}", crate::store::uid()))
    }

    #[test]
    fn download_keeps_existing_file_and_publishes_only_after_validation() {
        let dir = root();
        let url = "blob:https://example.org/r";
        let name = Path::new("axe.vpk");
        let stage = stage_download(&dir, name, url).unwrap();
        let old = dir.join(name);
        fs::write(&old, b"previous").unwrap();
        fs::write(&stage.file, VPK).unwrap();
        let published = publish(stage, &dir).unwrap();
        assert_eq!(published, dir.join("axe (1).vpk"));
        assert_eq!(fs::read(published).unwrap(), VPK);
        assert_eq!(fs::read(&old).unwrap(), b"previous");
        for bad in [&b""[..], &b"<html>Not found</html>"[..]] {
            let stage = stage_download(&dir, Path::new("bad.zip"), url).unwrap();
            fs::write(&stage.file, bad).unwrap();
            assert!(publish(stage, &dir).is_err());
            assert!(!dir.join("bad.zip").exists());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_duplicate_cannot_discard_the_other_download() {
        let dir = root();
        let url = "https://example.org/a.vpk";
        let mut transfers = Transfers::default();
        let mut first = dir.join("a.vpk");
        let mut second = first.clone();
        transfers.start(&dir, &mut first, url).unwrap();
        transfers.start(&dir, &mut second, url).unwrap();
        fs::write(&second, VPK).unwrap();
        assert!(matches!(
            transfers.finish(&dir, url, None, false),
            Some(BrowserEvent::Error(_))
        ));
        assert!(second.exists());
        assert!(matches!(
            transfers.finish(&dir, url, Some(second), true),
            Some(BrowserEvent::Downloaded(_))
        ));
        assert!(transfers.stages.is_empty());
        assert_eq!(fs::read(dir.join("a.vpk")).unwrap(), VPK);
        fs::remove_dir_all(dir).unwrap();
    }
}
