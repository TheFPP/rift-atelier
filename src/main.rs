#![windows_subsystem = "windows"]
mod bkb;
mod catalog;
mod catalog_browser;
mod collection;
mod deployment;
mod dialogs;
mod download;
mod extras;
mod home;
mod library;
mod native;
mod packages;
mod previews;
mod resources;
mod showcase;
mod store;
mod theme;
use eframe::egui::*;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
use store::{Installation, Item, State};
use theme::*;

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Home,
    Library,
    Profiles,
    Catalog,
    Settings,
}
impl Page {
    fn label(self) -> &'static str {
        match self {
            Self::Home => "Обзор",
            Self::Library => "Библиотека",
            Self::Profiles => "Профили",
            Self::Catalog => "Каталог",
            Self::Settings => "Настройки",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Library => "library",
            Self::Profiles => "profile",
            Self::Catalog => "catalog",
            Self::Settings => "settings",
        }
    }
}
struct Import {
    path: PathBuf,
    target: String,
    term: String,
    hits: Option<Vec<String>>,
}
enum Dialog {
    FromLink(String),
    Import(Import),
    Details(Item),
    NewProfile(String),
    Remove(String),
    Uninstall,
}
enum Done {
    Imported(Vec<Item>),
    Targets(Vec<String>),
    Details {
        id: String,
        cover: Option<(PathBuf, String)>,
        image: Option<ColorImage>,
        entries: Vec<String>,
    },
    Attached {
        id: String,
        cover: PathBuf,
        image: ColorImage,
    },
    Downloads(Vec<PathBuf>),
    Installed(Installation, usize, PathBuf),
    Removed,
    Exported(usize, PathBuf),
}
type Task = Result<Done, String>;
struct Atelier {
    state: State,
    dir: PathBuf,
    page: Page,
    showcase: showcase::Showcase,
    browser: catalog_browser::Browser,
    transfer: Option<std::sync::Arc<download::Progress>>,
    thumbs: collection::Thumbnails,
    bkb: bkb::Bkb,
    dialog: Option<Dialog>,
    game_found: bool,
    queue: VecDeque<PathBuf>,
    worker: Option<(String, Receiver<Task>)>,
    notice: Option<(String, bool, Instant)>,
    library_view: collection::LibraryView,
    settings_dir: String,
    settings_lang: String,
    settings_volume: u8,
    downloads: Vec<PathBuf>,
    preview: Option<TextureHandle>,
    entries: Vec<String>,
    details_error: Option<String>,
    archive_filter: String,
    dialog_error: Option<String>,
    ctx: Context,
}
impl Atelier {
    fn new(cc: &eframe::CreationContext<'_>, mut state: State, dir: PathBuf) -> Self {
        let ctx = cc.egui_ctx.clone();
        theme::install(&ctx);
        let showcase = showcase::Showcase::new(dir.join("hero-portraits"));
        if state.dota.as_os_str().is_empty() {
            if let Some(p) = store::detect_game() {
                state.dota = p;
            }
        }
        let game_found = store::game_ok(&state.dota);
        let recovery = store::recover_installation(&mut state).err();
        Self {
            settings_dir: state.dota.to_string_lossy().into(),
            settings_lang: state.language.clone(),
            settings_volume: state.bkb_volume.min(100),
            state,
            dir,
            page: Page::Home,
            showcase,
            browser: Default::default(),
            transfer: None,
            thumbs: Default::default(),
            bkb: Default::default(),
            dialog: None,
            game_found,
            queue: VecDeque::new(),
            worker: None,
            notice: recovery.map(|s| (s, true, Instant::now())),
            library_view: Default::default(),
            downloads: vec![],
            preview: None,
            entries: vec![],
            details_error: None,
            archive_filter: String::new(),
            dialog_error: None,
            ctx,
        }
    }
    fn notice(&mut self, msg: impl Into<String>, err: bool) {
        self.notice = Some((msg.into(), err, Instant::now()));
        self.ctx.request_repaint_after(Duration::from_secs(7));
    }
    fn error(&mut self, msg: String) {
        if self.dialog.is_some() {
            self.dialog_error = Some(msg.clone());
        }
        self.notice(msg, true);
    }
    fn change(&mut self, f: impl FnOnce(&mut State)) -> bool {
        let mut updated = self.state.clone();
        f(&mut updated);
        match updated.save(&self.dir) {
            Ok(()) => {
                self.state = updated;
                true
            }
            Err(e) => {
                self.error(e);
                false
            }
        }
    }
    fn run(&mut self, title: &str, op: impl FnOnce() -> Task + Send + 'static) {
        if self.worker.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.worker = Some((title.into(), rx));
        self.dialog_error = None;
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(op)).unwrap_or_else(|_| {
                    Err("Операция прервана. Проверьте формат исходного файла.".into())
                });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    fn receive(&mut self) {
        let Some((_, rx)) = &self.worker else {
            return;
        };
        let msg = match rx.try_recv() {
            Ok(msg) => msg,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("Рабочий поток завершился.".into()),
        };
        self.worker = None;
        self.transfer = None;
        match msg {
            Err(e) => {
                if matches!(self.dialog, Some(Dialog::Details(_))) {
                    self.details_error = Some(e.clone());
                }
                self.error(e);
            }
            Ok(Done::Targets(v)) => {
                if let Some(Dialog::Import(i)) = &mut self.dialog {
                    i.hits = Some(v);
                }
            }
            Ok(Done::Details {
                id,
                cover,
                image,
                entries,
            }) => {
                if let Some(cover) = cover {
                    self.remember_cover(&id, cover);
                }
                self.preview =
                    image.map(|im| self.ctx.load_texture("preview", im, TextureOptions::LINEAR));
                self.entries = entries;
            }
            Ok(Done::Attached { id, cover, image }) => {
                self.remember_cover(&id, (cover, "Прикреплённый скриншот".into()));
                self.preview = Some(self.ctx.load_texture(
                    "preview",
                    image,
                    TextureOptions::LINEAR,
                ));
            }
            Ok(Done::Downloads(v)) => self.downloads = v,
            Ok(Done::Imported(items)) => {
                let n = items.len();
                if self.change(|s| {
                    if n == 1 {
                        s.profiles[s.selected].ids.push(items[0].id.clone());
                    }
                    s.items.extend(items);
                }) {
                    self.close();
                    self.page = Page::Library;
                    self.library_view = Default::default();
                    self.notice(
                        if n == 1 {
                            "Мод добавлен. Нажмите «Применить в Dota 2», чтобы записать профиль в игру.".into()
                        } else {
                            format!(
                                "Добавлено пакетов: {n}. Включите нужные варианты в библиотеке."
                            )
                        },
                        false,
                    );
                }
            }
            Ok(Done::Installed(i, n, path)) => {
                self.close();
                if self.change(|s| s.installed = Some(i)) {
                    self.notice(format!("Записан и проверен {} · ресурсов: {n}. Если Dota 2 открыта, перезапустите её.",library::shortname(&path)),false);
                }
            }
            Ok(Done::Removed) => {
                self.close();
                if self.change(|s| s.installed = None) {
                    self.notice("Установка отменена. Пакет Rift Atelier удалён.", false);
                }
            }
            Ok(Done::Exported(n, p)) => {
                self.notice(format!("Сохранено ресурсов: {n}. {}", p.display()), false)
            }
        }
    }
    fn close(&mut self) {
        let _ = native::sound(None);
        self.dialog = None;
        self.preview = None;
        self.entries.clear();
        self.details_error = None;
        self.dialog_error = None;
        self.archive_filter.clear();
    }
    fn pick(&mut self, k: usize) {
        let (pat, title) = match k {
            0 => (
                "*.png;*.tga;*.jpg;*.jpeg;*.vtex_c;*.vmat_c;*.vpk;*.zip",
                "Выберите текстуру или материал",
            ),
            1 => (
                "*.vmdl;*.vmdl_c;*.vpk;*.zip",
                "Выберите модель или пакет с модом",
            ),
            2 => (
                "*.wav;*.vsnd_c;*.vpk;*.zip",
                "Выберите звук или пакет с модом",
            ),
            4 => ("*.vpk;*.zip", "Выберите скачанный мод"),
            _ => (
                "*.vpk;*.zip;*.png;*.jpg;*.jpeg;*.tga;*.wav;*.vmdl;*.vtex_c;*.vmat_c;*.vmdl_c;*.vsnd_c",
                "Импортировать ресурс",
            ),
        };
        match native::pick_many(pat, title) {
            Ok(paths) => self.queue.extend(paths),
            Err(e) => self.error(e),
        }
    }
    fn next_import(&mut self) {
        if self.worker.is_some() || self.dialog.is_some() {
            return;
        }
        if let Some(path) = self.queue.pop_front() {
            let ext = store::ext(&path);
            let Some((_, dst)) = store::source_type(&ext) else {
                self.error(format!("Формат .{ext} не поддерживается. Для FBX сохраните модель как .vmdl в ModelDoc."));
                self.ctx.request_repaint();
                return;
            };
            if dst.is_empty() {
                let dir = self.dir.clone();
                let root = self.state.dota.clone();
                self.run("Импорт пакета", move || {
                    library::import(&dir, &root, &path, "").map(Done::Imported)
                });
            } else {
                let term = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_string();
                self.dialog = Some(Dialog::Import(Import {
                    target: store::suggest_target(&path, dst).unwrap_or_default(),
                    path,
                    term,
                    hits: None,
                }));
                self.dialog_error = None;
            }
        }
    }
    fn navigate(&mut self, page: Page) {
        self.page = page;
        if page == Page::Catalog && self.browser.local_files {
            self.refresh_downloads();
        }
    }
    fn refresh_downloads(&mut self) {
        let d = catalog::downloads_dir();
        self.run("Чтение загрузок", move || {
            library::get_files(&d.ok_or("Папка загрузок недоступна.")?).map(Done::Downloads)
        });
    }
    fn details(&mut self, id: &str) {
        if let Some(item) = self.state.items.iter().find(|i| i.id == id).cloned() {
            self.close();
            self.dialog = Some(Dialog::Details(item.clone()));
            self.run("Чтение ресурса", move || {
                let cover = if item.cover.as_ref().is_none_or(|(p, _)| !p.is_file())
                    && item.preview.is_none()
                {
                    previews::discover(
                        &item.file,
                        &item.file,
                        item.file
                            .parent()
                            .ok_or("Не удалось найти папку ресурса.")?,
                    )
                    .ok()
                    .flatten()
                } else {
                    None
                };
                let display = cover
                    .as_ref()
                    .or(item.cover.as_ref())
                    .map(|(p, _)| p)
                    .or(item.preview.as_ref().filter(|p| store::ext(p) != "wav"));
                let image = display.and_then(|path| library::texture_preview(path).ok());
                let entries = if item.target.is_empty() {
                    packages::inspect(&item.file)?
                } else {
                    vec![]
                };
                Ok(Done::Details {
                    id: item.id,
                    cover,
                    image,
                    entries,
                })
            });
        }
    }
    fn remember_cover(&mut self, id: &str, cover: (PathBuf, String)) {
        if self.change(|s| {
            if let Some(i) = s.items.iter_mut().find(|i| i.id == id) {
                i.cover = Some(cover.clone());
            }
        }) && let Some(Dialog::Details(item)) = &mut self.dialog
            && item.id == id
        {
            let previous = item.cover.replace(cover.clone());
            if let Some((old, _)) = previous
                && old != cover.0
                && old.parent() == item.file.parent()
                && old
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("preview-") || n.starts_with("cover-"))
                && !self
                    .state
                    .items
                    .iter()
                    .any(|i| i.cover.as_ref().is_some_and(|(p, _)| *p == old))
            {
                let _ = std::fs::remove_file(old);
            }
        }
    }
    fn action(&mut self, a: collection::Action) {
        use collection::Action::*;
        match a {
            Import => self.pick(3),
            OpenLibrary => {
                self.library_view = Default::default();
                self.navigate(Page::Library);
            }
            Details(id) => self.details(&id),
            Delete(id) => self.dialog = Some(Dialog::Remove(id)),
            Toggle(id) => {
                self.change(|s| s.toggle(&id));
            }
            NewProfile => self.dialog = Some(Dialog::NewProfile(String::new())),
            Select(n) => {
                self.change(|s| {
                    if n < s.profiles.len() {
                        s.selected = n;
                    }
                });
            }
            Move(a, b) => {
                self.change(|s| {
                    let ids = &mut s.profiles[s.selected].ids;
                    if a < ids.len() && b < ids.len() {
                        let id = ids.remove(a);
                        ids.insert(b, id);
                    }
                });
            }
            Export => {
                match library::inputs(&self.state.profiles[self.state.selected], &self.state.items)
                {
                    Ok(inputs) => {
                        if let Some(p) = native::choose("*.vpk", "Сохранить новый VPK", true)
                        {
                            self.run("Сборка VPK", move || {
                                packages::export(&inputs, &p).map(|n| Done::Exported(n, p))
                            });
                        }
                    }
                    Err(e) => self.error(e),
                }
            }
            Install => {
                if !self.game_found {
                    self.page = Page::Settings;
                    self.error("Укажите папку Dota 2 перед установкой.".into());
                } else if self
                    .state
                    .installed
                    .as_ref()
                    .is_some_and(|i| i.root != self.state.dota || i.language != self.state.language)
                {
                    self.error(
                        "Сначала отмените предыдущую установку в другом языковом каталоге.".into(),
                    );
                } else {
                    self.apply_profile();
                }
            }
            Uninstall => self.dialog = Some(Dialog::Uninstall),
        }
    }
    fn apply_profile(&mut self) {
        let state = self.state.clone();
        self.run(
            "Запись и проверка профиля в папке Dota 2",
            move || {
                deployment::apply(&state)
                    .map(|(record, count, path)| Done::Installed(record, count, path))
            },
        );
    }
    fn undo(&mut self) {
        let (root, lang) = self
            .state
            .installed
            .as_ref()
            .map(|i| (i.root.clone(), i.language.clone()))
            .unwrap_or_else(|| (self.state.dota.clone(), self.state.language.clone()));
        self.run("Отмена установки", move || {
            packages::uninstall(&root, &lang).map(|_| Done::Removed)
        });
    }
    fn sidebar(&mut self, ctx: &Context) {
        let w = if ctx.screen_rect().width() < 935. {
            168.
        } else {
            201.
        };
        SidePanel::left("rail")
            .frame(
                Frame::NONE
                    .fill(if self.bkb.active() {
                        Color32::from_rgb(57, 40, 9)
                    } else {
                        SIDE
                    })
                    .stroke(Stroke::new(1_f32, LINE)),
            )
            .exact_width(w)
            .resizable(false)
            .show(ctx, |ui| {
                if self.worker.is_some() {
                    ui.disable();
                }
                let (head, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 84.), Sense::hover());
                let p = ui.painter();
                mark(
                    p,
                    Rect::from_min_size(head.min + vec2(16., 20.), vec2(39., 39.)),
                );
                p.text(
                    head.min + vec2(62., 14.),
                    Align2::LEFT_TOP,
                    "RIFT",
                    bold(25.),
                    TEXT,
                );
                p.text(
                    head.min + vec2(64., 45.),
                    Align2::LEFT_TOP,
                    "A T E L I E R",
                    mono(10.),
                    MUTED,
                );
                p.line_segment(
                    [
                        head.left_bottom() + vec2(16., -1.),
                        head.right_bottom() - vec2(16., 1.),
                    ],
                    Stroke::new(1_f32, LINE),
                );
                Frame::NONE
                    .inner_margin(Margin::symmetric(13, 12))
                    .show(ui, |ui| {
                        ui.set_width(w - 26.);
                        kicker(ui, "МАСТЕРСКАЯ");
                        ui.add_space(4.);
                        for page in [Page::Home, Page::Library, Page::Profiles] {
                            if nav(ui, page.icon(), page.label(), self.page == page) {
                                self.navigate(page);
                            }
                        }
                        ui.add_space(19.);
                        kicker(ui, "ИСТОЧНИКИ");
                        ui.add_space(4.);
                        if nav(ui, "catalog", "Dota2PornFx", self.page == Page::Catalog) {
                            self.navigate(Page::Catalog);
                        }
                        ui.add_space(14.);
                        let now = ctx.input(|i| i.time);
                        let active = self.bkb.active();
                        let countdown = self.bkb.countdown(now);
                        let label = match countdown {
                            Some((left, _)) if active => format!("БКБ · {left} с"),
                            Some((left, _)) => format!("БКБ · КД {left} с"),
                            None => "БКБ".into(),
                        };
                        if ui
                            .add_enabled(
                                countdown.is_none(),
                                Button::new(
                                    RichText::new(label).font(bold(14.)).color(if active {
                                        Color32::BLACK
                                    } else {
                                        GOLD
                                    }),
                                )
                                .min_size(vec2(ui.available_width(), 29.))
                                .fill(if active {
                                    Color32::from_rgb(251, 196, 55)
                                } else {
                                    Color32::from_rgb(54, 44, 27)
                                }),
                            )
                            .on_hover_text(
                                "Первое применение длится 9 секунд, второе — 8, остальные — 7. Перезарядка — 95 секунд с нажатия.",
                            )
                            .clicked()
                            && self.bkb.activate(
                                now,
                                ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
                            )
                        {
                            native::play_bkb(self.state.dota.clone(), self.state.bkb_volume);
                            ctx.send_viewport_cmd(ViewportCommand::Maximized(true));
                            ctx.request_repaint();
                        }
                        if ui
                            .add_sized(
                                [ui.available_width(), 29.],
                                Button::new("Поддержать автора"),
                            )
                            .on_hover_text("Открыть DonationAlerts в браузере")
                            .clicked()
                            && let Err(e) = std::process::Command::new("explorer.exe")
                                .arg("https://www.donationalerts.com/r/the_fpp")
                                .spawn()
                        {
                            self.error(format!("Не удалось открыть DonationAlerts. {e}"));
                        }
                    });
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    Frame::NONE
                        .inner_margin(Margin::symmetric(15, 14))
                        .show(ui, |ui| {
                            ui.set_width(w - 30.);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("RIFT ATELIER").font(mono(10.)).color(MUTED),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                                            .font(mono(10.))
                                            .color(MUTED),
                                    )
                                });
                            });
                            ui.separator();
                            if nav(ui, "settings", "Настройки", self.page == Page::Settings)
                            {
                                self.navigate(Page::Settings);
                            }
                        });
                });
            });
    }
    fn body(&mut self, ctx: &Context) {
        CentralPanel::default()
            .frame(Frame::NONE.fill(if self.bkb.active() {
                Color32::from_rgb(76, 53, 11)
            } else {
                BG
            }))
            .show(ctx, |ui| {
                if self.worker.is_some() {
                    ui.disable();
                }
                let margin = if ui.available_width() < 700. { 18 } else { 26 };
                Frame::NONE
                    .inner_margin(Margin::symmetric(margin, 0))
                    .show(ui, |ui| {
                        self.page_header(ui);
                        self.page_content(ui);
                    });
            });
    }

    fn page_header(&mut self, ui: &mut Ui) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 63.), Sense::hover());
        let p = ui.painter();
        let left = bar.left_center();
        p.text(left, Align2::LEFT_CENTER, "RIFT", mono(12.), MUTED);
        p.text(
            left + vec2(43., 0.),
            Align2::LEFT_CENTER,
            "/",
            mono(12.),
            Color32::DARK_GRAY,
        );
        p.text(
            left + vec2(66., -1.),
            Align2::LEFT_CENTER,
            self.page.label(),
            bold(14.),
            TEXT,
        );

        let (status, width, color) = if self.game_found {
            ("Dota 2 найдена", 119., MINT)
        } else {
            ("Указать путь к Dota 2", 161., GOLD)
        };
        let x = bar.right() - width;
        p.circle_filled(pos2(x, bar.center().y), 3., color);
        p.text(
            pos2(x + 11., bar.center().y - 1.),
            Align2::LEFT_CENTER,
            status,
            FontId::proportional(13.),
            MUTED,
        );
        let target = Rect::from_min_max(pos2(x - 8., bar.min.y + 10.), bar.max - vec2(0., 10.));
        if ui
            .interact(target, Id::new("game_settings"), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
        {
            self.navigate(Page::Settings);
        }
    }

    fn page_content(&mut self, ui: &mut Ui) {
        match self.page {
            Page::Catalog => {
                if let Some(action) = extras::catalog_page(ui, &self.downloads, &mut self.browser) {
                    self.catalog_action(action);
                }
            }
            Page::Settings => {
                if let Some(action) = extras::settings_page(
                    ui,
                    &mut self.settings_dir,
                    &mut self.settings_lang,
                    &mut self.settings_volume,
                    self.state.installed.as_ref(),
                ) {
                    self.settings_action(action);
                }
                if self.settings_volume != self.state.bkb_volume
                    && !ui.input(|i| i.pointer.any_down())
                {
                    let volume = self.settings_volume;
                    if !self.change(|state| state.bkb_volume = volume) {
                        self.settings_volume = self.state.bkb_volume;
                    }
                }
            }
            page => {
                if let Some(action) = collection::installation_bar(ui, &self.state) {
                    self.action(action);
                }
                ScrollArea::vertical()
                    .id_salt(format!("page_{}", page.label()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match page {
                            Page::Home => {
                                if let Some(action) =
                                    home::show(ui, &mut self.showcase, &self.state)
                                {
                                    match action {
                                        home::Action::Browse(k) => self.pick(k),
                                        home::Action::Library => self.navigate(Page::Library),
                                        home::Action::Profiles => self.navigate(Page::Profiles),
                                        home::Action::Detail(id) => self.details(&id),
                                        home::Action::Catalog => self.navigate(Page::Catalog),
                                    }
                                }
                            }
                            Page::Library => {
                                if let Some(action) = collection::library_page(
                                    ui,
                                    &self.state,
                                    &mut self.library_view,
                                    &mut self.thumbs,
                                ) {
                                    self.action(action);
                                }
                            }
                            Page::Profiles => {
                                if let Some(action) = collection::profiles_page(ui, &self.state) {
                                    self.action(action);
                                }
                            }
                            _ => {}
                        }
                        ui.add_space(12.);
                    });
            }
        }
    }

    fn catalog_action(&mut self, action: extras::CatalogAction) {
        match action {
            extras::CatalogAction::Open(url) => self.browser.navigate(url),
            extras::CatalogAction::FromLink => self.dialog = Some(Dialog::FromLink(String::new())),
            extras::CatalogAction::Refresh => self.refresh_downloads(),
            extras::CatalogAction::Import(path) => self.queue.push_back(path),
            extras::CatalogAction::Browse => self.pick(4),
        }
    }

    fn settings_action(&mut self, action: extras::SettingsAction) {
        match action {
            extras::SettingsAction::Browse => {
                if let Some(exe) =
                    native::choose("dota2.exe", "Выберите dota2.exe из game/bin/win64", false)
                    && let Some(root) = exe.ancestors().nth(4)
                {
                    self.settings_dir = root.to_string_lossy().into();
                }
            }
            extras::SettingsAction::Save => {
                let path = PathBuf::from(self.settings_dir.trim().trim_matches('"'));
                if !store::game_ok(&path) {
                    self.error("В этой папке нет game/dota/pak01_dir.vpk. Выберите корневую папку dota 2 beta.".into());
                    return;
                }
                let language = self.settings_lang.clone();
                if self.change(|s| {
                    s.dota = path;
                    s.language = language;
                }) {
                    self.game_found = true;
                    self.notice("Настройки сохранены.", false);
                }
            }
            extras::SettingsAction::Uninstall => self.dialog = Some(Dialog::Uninstall),
        }
    }
}
impl eframe::App for Atelier {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        if let Some(previous) = self.bkb.advance(now) {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(previous));
        }
        self.receive();
        self.browser.begin_frame();
        for event in self.browser.take_events() {
            match event {
                catalog_browser::BrowserEvent::Downloaded(path) => self.queue.push_back(path),
                catalog_browser::BrowserEvent::Error(e) => self.error(e),
            }
        }
        if (self.worker.is_some() || self.browser.download_count() > 0)
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.notice("Дождитесь завершения операции перед закрытием.", false);
        }
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect::<Vec<_>>()
        });
        self.queue.extend(dropped);
        self.next_import();
        if self.dialog.is_none() && self.worker.is_none() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::O)) {
                self.pick(3);
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::F)) {
                self.navigate(Page::Library);
                ctx.memory_mut(|m| m.request_focus(Id::new(collection::SEARCH_FIELD)));
            }
        }
        self.sidebar(ctx);
        if let Some((_, next_change)) = self.bkb.countdown(now) {
            ctx.request_repaint_after(next_change);
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, err, t)| !*err && t.elapsed() > Duration::from_secs(7))
        {
            self.notice = None;
        }
        if self.notice.is_some() || self.worker.is_some() {
            TopBottomPanel::bottom("feedback")
                .frame(
                    Frame::NONE
                        .fill(SIDE)
                        .inner_margin(Margin::symmetric(22, 8)),
                )
                .show(ctx, |ui| {
                    if let Some((s, _)) = &self.worker {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(muted(s));
                        });
                    } else if let Some((msg, err, _)) = &self.notice {
                        let s = msg.clone();
                        let c = if *err { RED } else { MINT };
                        ui.horizontal(|ui| {
                            if ui.small_button("×").on_hover_text("Скрыть").clicked() {
                                self.notice = None;
                            }
                            ui.colored_label(c, s);
                        });
                    }
                });
        }
        self.body(ctx);
        self.dialog_ui(ctx);
        if self.bkb.active() {
            let p = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("bkb-tint")));
            let r = ctx.screen_rect();
            p.rect_filled(r, 0., Color32::from_rgba_unmultiplied(255, 189, 24, 48));
            p.rect_stroke(
                r.shrink(1.),
                0.,
                Stroke::new(3_f32, Color32::from_rgb(255, 204, 62)),
                StrokeKind::Inside,
            );
        }
        self.next_import();
        if self.page != Page::Catalog || self.dialog.is_some() || self.worker.is_some() {
            self.browser.hide_for_frame();
        }
        self.browser.sync(frame, ctx, &self.dir);
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let p = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("drop")));
            let r = ctx.screen_rect();
            p.rect_filled(r, 0., Color32::from_black_alpha(205));
            p.rect_stroke(
                r.shrink(18.),
                8.,
                Stroke::new(2_f32, RED),
                StrokeKind::Inside,
            );
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                "Отпустите файл для импорта",
                bold(27.),
                TEXT,
            );
        }
    }
}
fn main() {
    let Some(_instance) = native::launch_instance() else {
        return;
    };
    let dir = store::data_dir();
    let state = match State::load(&dir) {
        Ok(s) => s,
        Err(e) => {
            native::message(&format!("{e}\n{}", dir.display()));
            return;
        }
    };
    let opt = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title(native::WINDOW_TITLE)
            .with_inner_size([1130., 765.])
            .with_min_inner_size([830., 595.])
            .with_icon(theme::icon_data()),
        renderer: eframe::Renderer::Glow,
        multisampling: 0,
        vsync: true,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "Rift Atelier",
        opt,
        Box::new(move |cc| Ok(Box::new(Atelier::new(cc, state, dir)))),
    ) {
        native::message(&format!("Не удалось открыть Rift Atelier.\n{e}"));
    }
}
