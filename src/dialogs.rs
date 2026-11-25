use std::path::PathBuf;

use eframe::egui::*;

use crate::{Atelier, Dialog, Done, Import, library, native, resources, store, theme::*};
use store::{Item, Profile};

enum DialogOp {
    Close,
    Download(String),
    Attach(Item),
    Search(String, String),
    Import(PathBuf, String),
    NewProfile(String),
    Delete(String),
    Undo,
    Play(Option<PathBuf>),
}

impl Dialog {
    fn title(&self) -> &'static str {
        match self {
            Self::FromLink(_) => "Скачать мод по ссылке",
            Self::Import(_) => "Назначить замену",
            Self::Details(_) => "Сведения о ресурсе",
            Self::NewProfile(_) => "Новый профиль",
            Self::Remove(_) => "Удалить из библиотеки?",
            Self::Uninstall => "Снять установленный профиль?",
        }
    }
}

fn heading(ui: &mut Ui, title: &str) -> bool {
    ui.horizontal(|ui| {
        ui.label(strong(title, 24.));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.button("×").on_hover_text("Закрыть").clicked()
        })
        .inner
    })
    .inner
}

fn target_search(
    ui: &mut Ui,
    form: &mut Import,
    game_found: bool,
    extension: &str,
) -> Option<DialogOp> {
    let search = ui
        .horizontal(|ui| {
            let width = (ui.available_width() - 98.).max(150.);
            let field = TextEdit::singleline(&mut form.term)
                .desired_width(width)
                .hint_text("Поиск в архиве игры");
            let response = ui.add(field);
            let enter = response.lost_focus() && ui.input(|k| k.key_pressed(Key::Enter));
            let enabled = game_found && !form.term.trim().is_empty();
            let clicked = ui.add_enabled(enabled, Button::new("Найти")).clicked();
            enabled && (clicked || enter)
        })
        .inner;
    if !game_found {
        ui.label(muted(
            "Для поиска укажите папку Dota 2 в настройках. Можно ввести путь вручную.",
        ));
    }
    if let Some(hits) = &form.hits {
        if hits.is_empty() {
            ui.label(muted("Совпадений нет. Попробуйте другое имя."));
        } else {
            let more = if hits.len() == 60 { "+" } else { "" };
            ui.label(muted(&format!(
                "Найдено: {}{more} · нажмите на нужный путь",
                hits.len()
            )));
            ScrollArea::vertical()
                .id_salt("paths")
                .max_height(146.)
                .show(ui, |ui| {
                    for hit in hits {
                        let label = RichText::new(hit).font(mono(12.));
                        if ui.selectable_label(form.target == *hit, label).clicked() {
                            form.target = hit.clone();
                        }
                    }
                });
        }
    }
    search.then(|| DialogOp::Search(extension.into(), form.term.clone()))
}

fn import_form(ui: &mut Ui, form: &mut Import, game_found: bool) -> Option<DialogOp> {
    let source_extension = store::ext(&form.path);
    let (kind, extension) = store::source_type(&source_extension).unwrap_or(("Ресурс", ""));
    ui.label(strong(&library::shortname(&form.path), 17.));
    ui.label(muted(&format!("{kind}   →   .{extension}")));
    ui.separator();
    ui.label(strong("Какой игровой файл заменить?", 16.));
    ui.label(muted(
        "Введите имя героя или фрагмент пути, например axe или sounds/ui.",
    ));
    let mut action = target_search(ui, form, game_found, extension);

    ui.add_space(4.);
    ui.label(strong("Путь к ресурсу в Dota 2", 15.));
    ui.add(
        TextEdit::singleline(&mut form.target)
            .desired_width(f32::INFINITY)
            .hint_text(format!("…/имя.{extension}")),
    );
    if source_extension == "vmdl" {
        ui.label(muted("Исходная VMDL должна находиться в проекте Workshop Tools. Свои материалы и текстуры добавьте отдельно."));
    }
    ui.add_space(6.);
    ui.horizontal(|ui| {
        let valid = store::validate_target(form.target.trim(), extension).is_ok();
        let label = RichText::new("Добавить в профиль")
            .color(Color32::BLACK)
            .font(bold(14.));
        let add = Button::new(label).fill(RED).min_size(vec2(0., 34.));
        if ui.add_enabled(valid, add).clicked() {
            action = Some(DialogOp::Import(form.path.clone(), form.target.clone()));
        }
        if button(ui, "Отмена", false).clicked() {
            action = Some(DialogOp::Close);
        }
    });
    action
}

fn archive_index(ui: &mut Ui, entries: &[String], filter: &mut String) {
    ui.separator();
    ui.add(
        TextEdit::singleline(filter)
            .hint_text("Найти в составе VPK")
            .desired_width(f32::INFINITY),
    );
    let query = filter.to_lowercase();
    ScrollArea::vertical()
        .id_salt("vpk_tree")
        .max_height(225.)
        .show(ui, |ui| {
            let matching = entries
                .iter()
                .filter(|s| query.is_empty() || s.to_lowercase().contains(&query));
            for entry in matching.take(240) {
                ui.add(Label::new(RichText::new(entry).font(mono(11.5))).wrap());
            }
        });
}

fn item_details(
    ui: &mut Ui,
    item: &Item,
    preview: Option<&TextureHandle>,
    entries: &[String],
    filter: &mut String,
) -> Option<DialogOp> {
    ui.label(strong(&item.name, 19.));
    let size = store::fmt_size(item.size);
    ui.label(muted(&format!(
        "{}  ·  {size}  ·  ресурсов: {}",
        item.kind, item.count
    )));
    if !item.target.is_empty() {
        ui.separator();
        kicker(ui, "ИГРОВОЙ ПУТЬ");
        ui.add(
            Label::new(RichText::new(&item.target).font(mono(13.)))
                .selectable(true)
                .wrap(),
        );
    }
    if let Some(tex) = preview {
        let size = tex.size_vec2();
        let height = ui.ctx().screen_rect().height() * if entries.is_empty() { 0.38 } else { 0.25 };
        let scale = (ui.available_width() / size.x).min(height / size.y).min(1.);
        ui.add(Image::new((tex.id(), size * scale)).corner_radius(4.));
        if let Some((_, caption)) = &item.cover {
            ui.label(muted(caption));
        }
    }
    let mut action = None;
    if !matches!(item.kind.as_str(), "Текстура" | "Звук") || item.cover.is_some() {
        if preview.is_none() {
            ui.label(muted("У этого файла пока нет изображения."));
        }
        if button(
            ui,
            if preview.is_some() {
                "Заменить картинку…"
            } else {
                "Прикрепить скриншот…"
            },
            false,
        )
        .clicked()
        {
            action = Some(DialogOp::Attach(item.clone()));
        }
    }
    if let Some(path) = item.preview.as_ref().filter(|p| store::ext(p) == "wav") {
        ui.horizontal(|ui| {
            if button(ui, "Воспроизвести WAV", true).clicked() {
                action = Some(DialogOp::Play(Some(path.clone())));
            }
            if button(ui, "Стоп", false).clicked() {
                action = Some(DialogOp::Play(None));
            }
        });
    }
    if item.target.is_empty() && !entries.is_empty() {
        archive_index(ui, entries, filter);
    }
    action
}

fn download_form(ui: &mut Ui, url: &mut String) -> Option<DialogOp> {
    ui.label(muted(
        "Вставьте прямую ссылку на ZIP или VPK. После скачивания мод появится в библиотеке.",
    ));
    let field = ui.add(
        TextEdit::singleline(url)
            .hint_text("https://…/model.zip")
            .desired_width(f32::INFINITY),
    );
    let valid = crate::download::address(url).is_ok();
    let label = RichText::new("Скачать и добавить")
        .font(bold(14.))
        .color(Color32::BLACK);
    let clicked = ui
        .add_enabled(valid, Button::new(label).fill(RED))
        .clicked();
    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    (valid && (clicked || enter)).then(|| DialogOp::Download(url.clone()))
}
fn profile_form(ui: &mut Ui, name: &mut String, profiles: &[Profile]) -> Option<DialogOp> {
    ui.label(muted(
        "Создайте набор замен. Ресурсы для него можно включить в библиотеке.",
    ));
    let field = ui.add(
        TextEdit::singleline(name)
            .hint_text("Название профиля")
            .char_limit(48)
            .desired_width(f32::INFINITY),
    );
    let trimmed = name.trim();
    let valid = !trimmed.is_empty()
        && !profiles
            .iter()
            .any(|p| p.name.to_lowercase() == trimmed.to_lowercase());
    let clicked = ui
        .add_enabled(valid, Button::new("Создать профиль"))
        .clicked();
    let enter = field.lost_focus() && ui.input(|k| k.key_pressed(Key::Enter));
    (valid && (clicked || enter)).then(|| DialogOp::NewProfile(trimmed.into()))
}

fn confirm_removal(ui: &mut Ui, id: &str, items: &[Item]) -> Option<DialogOp> {
    if let Some(item) = items.iter().find(|item| item.id == id) {
        ui.label(format!(
            "«{}» будет удалён из библиотеки и всех профилей. \
                          Исходный файл останется у вас.",
            item.name
        ));
    }
    button(ui, "Удалить ресурс", true)
        .clicked()
        .then(|| DialogOp::Delete(id.into()))
}

fn confirm_uninstall(ui: &mut Ui) -> Option<DialogOp> {
    ui.label("Закройте Dota 2. Приложение сверит контрольную сумму своего пакета перед удалением.");
    button(ui, "Снять профиль", true)
        .clicked()
        .then_some(DialogOp::Undo)
}

impl Atelier {
    pub(super) fn dialog_ui(&mut self, ctx: &Context) {
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let busy = self.worker.is_some();
        let modal = Modal::new(Id::new("atelier_dialog"))
            .frame(box_frame().inner_margin(23))
            .backdrop_color(Color32::from_black_alpha(174));
        let response = modal.show(ctx, |ui| {
            let screen = ctx.screen_rect().size();
            ui.set_width((screen.x - 92.).clamp(340., 555.));
            ui.set_max_height((screen.y - 68.).max(100.));
            let close = ui
                .add_enabled_ui(!busy, |ui| heading(ui, dialog.title()))
                .inner;
            ui.add_space(6.);
            let id = Id::new(("modal-body", dialog.title()));
            let id = match dialog {
                Dialog::Details(item) => id.with(&item.id),
                Dialog::Import(form) => id.with(&form.path),
                _ => id,
            };
            let action = ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, true])
                .min_scrolled_height(0.)
                .max_height((screen.y - 118.).max(80.))
                .show(ui, |ui| {
                    let action = ui
                        .add_enabled_ui(!busy, |ui| match dialog {
                            Dialog::Import(form) => import_form(ui, form, self.game_found),
                            Dialog::FromLink(url) => download_form(ui, url),
                            Dialog::Details(item) => item_details(
                                ui,
                                item,
                                self.preview.as_ref(),
                                &self.entries,
                                &mut self.archive_filter,
                            ),
                            Dialog::NewProfile(name) => {
                                profile_form(ui, name, &self.state.profiles)
                            }
                            Dialog::Remove(id) => confirm_removal(ui, id, &self.state.items),
                            Dialog::Uninstall => confirm_uninstall(ui),
                        })
                        .inner;
                    if let Some((label, _)) = &self.worker {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(muted(label));
                        });
                        if let Some(progress) = &self.transfer {
                            use std::sync::atomic::Ordering;
                            ui.horizontal(|ui| {
                                let bytes = progress.bytes.load(Ordering::Relaxed);
                                ui.label(muted(&format!("Получено {}", store::fmt_size(bytes))));
                                if progress.importing.load(Ordering::Acquire) {
                                    ui.label(muted("Добавляем в библиотеку…"));
                                } else if ui.button("Отменить загрузку").clicked() {
                                    progress.cancel.store(true, Ordering::Relaxed);
                                }
                            });
                            ctx.request_repaint_after(std::time::Duration::from_millis(250));
                        }
                    } else if let Some(error) =
                        self.dialog_error.as_ref().or(self.details_error.as_ref())
                    {
                        ui.add(Label::new(RichText::new(error).color(RED)).wrap());
                    }
                    action
                })
                .inner;
            action.or(close.then_some(DialogOp::Close))
        });
        let dismiss = !busy && response.should_close();
        if let Some(action) = response.inner.or(dismiss.then_some(DialogOp::Close)) {
            self.dialog_action(action);
        }
    }
}

impl Atelier {
    fn dialog_action(&mut self, op: DialogOp) {
        match op {
            DialogOp::Close => self.close(),
            DialogOp::Download(url) => {
                let root = self.state.dota.clone();
                let dir = self.dir.clone();
                let progress = std::sync::Arc::new(crate::download::Progress::default());
                self.transfer = Some(progress.clone());
                self.run(
                    "Загрузка и проверка файла",
                    move || {
                        crate::download::import_url(&dir, &root, &url, &progress)
                            .map(Done::Imported)
                    },
                );
            }
            DialogOp::Attach(item) => {
                match native::pick_many("*.png;*.jpg;*.jpeg;*.webp", "Выберите скриншот")
                {
                    Ok(paths) if !paths.is_empty() => {
                        let first = paths[0].clone();
                        self.run("Подготовка скриншота", move || {
                            let cover = crate::previews::attach(
                                &first,
                                item.file.parent().ok_or("Нет папки ресурса.")?,
                            )?;
                            let image = library::texture_preview(&cover)?;
                            Ok(Done::Attached {
                                id: item.id,
                                cover,
                                image,
                            })
                        });
                    }
                    Ok(_) => {}
                    Err(error) => self.error(error),
                }
            }
            DialogOp::Search(ext, q) => {
                let root = self.state.dota.clone();
                self.run("Поиск в архиве Dota 2", move || {
                    resources::find_targets(&root, &ext, &q, 60).map(Done::Targets)
                });
            }
            DialogOp::Import(path, target) => {
                let dir = self.dir.clone();
                let root = self.state.dota.clone();
                self.run("Подготовка ресурса", move || {
                    library::import(&dir, &root, &path, &target).map(Done::Imported)
                });
            }
            DialogOp::NewProfile(name) => {
                if self.change(|s| {
                    s.profiles.push(Profile { name, ids: vec![] });
                    s.selected = s.profiles.len() - 1;
                }) {
                    self.close();
                    self.notice("Профиль создан. Выберите ресурсы в библиотеке.", false);
                }
            }
            DialogOp::Delete(id) => {
                let Some(removed) = self.state.items.iter().find(|i| i.id == id).cloned() else {
                    return;
                };
                if self.change(|s| {
                    s.items.retain(|i| i.id != id);
                    for p in &mut s.profiles {
                        p.ids.retain(|s| s != &id);
                    }
                }) {
                    self.close();
                    library::cleanup_removed(&self.dir, &removed, &self.state.items);
                    self.notice("Ресурс удалён из библиотеки.", false);
                }
            }
            DialogOp::Undo => self.undo(),
            DialogOp::Play(path) => {
                if let Err(e) = native::sound(path.as_deref()) {
                    self.error(e);
                }
            }
        }
    }
}
