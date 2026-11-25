use crate::{catalog, catalog_browser::Browser, store::Installation, theme::*};
use eframe::egui::*;
use std::path::PathBuf;

pub enum CatalogAction {
    Open(&'static str),
    FromLink,
    Refresh,
    Import(PathBuf),
    Browse,
}

pub enum SettingsAction {
    Browse,
    Save,
    Uninstall,
}

fn section_title(ui: &mut Ui, title: &str) {
    ui.label(strong(title, 20.));
}

pub fn catalog_page(
    ui: &mut Ui,
    downloads: &[PathBuf],
    browser: &mut Browser,
) -> Option<CatalogAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        title(ui, "Каталог модов");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if button(
                ui,
                if browser.local_files {
                    "К моделям"
                } else {
                    "Мои загрузки"
                },
                false,
            )
            .clicked()
            {
                browser.local_files = !browser.local_files;
                if browser.local_files {
                    action = Some(CatalogAction::Refresh);
                }
            }
            if button(ui, "Скачать по ссылке", false).clicked() {
                action = Some(CatalogAction::FromLink);
            }
        });
    });
    if browser.local_files {
        return download_list(ui, downloads).or(action);
    }
    ui.add_space(3.);
    ui.horizontal_wrapped(|ui| {
        for (label, url) in [
            ("Модели", catalog::SECTIONS[0].1),
            ("Предметы", catalog::SECTIONS[1].1),
            ("Звуки", catalog::SECTIONS[3].1),
            ("Все разделы", catalog::SITE),
        ] {
            if button(ui, label, false).clicked() {
                action = Some(CatalogAction::Open(url));
            }
        }
        if ui.button("↻").on_hover_text("Обновить страницу").clicked() {
            browser.reload();
        }
    });
    let count = browser.download_count();
    if count > 0 {
        ui.label(RichText::new(format!("Загружается файлов: {count}")).color(MINT));
    } else {
        ui.label(muted(
            "Выберите мод на странице Dota2PornFx. Скачанный VPK или ZIP появится в библиотеке.",
        ));
    }
    let (rect, _) = ui.allocate_exact_size(ui.available_size().max(vec2(1., 1.)), Sense::hover());
    ui.painter().rect_filled(rect, 4., CARD);
    if browser.failed() {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Страница недоступна. Нажмите ↻, чтобы повторить.",
            bold(15.),
            MUTED,
        );
    } else if browser.loading() {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Открываем Dota2PornFx…",
            bold(15.),
            MUTED,
        );
    }
    browser.show_at(rect);
    action
}

fn download_list(ui: &mut Ui, downloads: &[PathBuf]) -> Option<CatalogAction> {
    let mut action = None;
    ScrollArea::vertical()
        .id_salt("catalog-downloads")
        .show(ui, |ui| {
            ui.add_space(17.);
            ui.horizontal(|ui| {
                section_title(ui, "Недавние загрузки");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, "Обновить", false)
                        .on_hover_text("Проверить папку «Загрузки»")
                        .clicked()
                    {
                        action = Some(CatalogAction::Refresh);
                    }
                });
            });
            ui.label(muted(
                "Выберите готовый VPK или ZIP. Пустые и незавершённые загрузки скрыты.",
            ));
            ui.add_space(4.);
            if downloads.is_empty() {
                box_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(strong("В «Загрузках» пока нет VPK или ZIP", 16.));
                    ui.label(muted(
                        "Если файл сохранён в другом месте, выберите его вручную.",
                    ));
                });
            } else {
                for file in downloads {
                    let name = file.file_name().unwrap_or_default().to_string_lossy();
                    let ext = file
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_uppercase();
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 41.), Sense::click());
                    let p = ui.painter_at(rect);
                    if response.hovered() {
                        p.rect_filled(rect, 4., CARD);
                    }
                    p.hline(rect.x_range(), rect.bottom(), Stroke::new(1_f32, LINE));
                    p.text(
                        rect.left_center() + vec2(9., 0.),
                        Align2::LEFT_CENTER,
                        ext,
                        mono(11.),
                        MINT,
                    );
                    let label_rect = rect.shrink2(vec2(54., 0.));
                    p.with_clip_rect(label_rect.intersect(ui.clip_rect())).text(
                        label_rect.left_center(),
                        Align2::LEFT_CENTER,
                        name.as_ref(),
                        bold(14.),
                        TEXT,
                    );
                    paint_icon(&p, rect.right_center() - vec2(17., 0.), "plus", 17., MINT);
                    if response
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(format!("Добавить «{name}» в библиотеку"))
                        .clicked()
                    {
                        action = Some(CatalogAction::Import(file.clone()));
                    }
                }
            }
            ui.add_space(5.);
            if button(ui, "Выбрать VPK или ZIP…", false).clicked() {
                action = Some(CatalogAction::Browse);
            }
            ui.add_space(12.);
        });
    action
}

const LANGUAGES: &[(&str, &str)] = &[
    ("russian", "Русский"),
    ("koreana", "Корейский"),
    ("schinese", "Китайский (упрощённый)"),
];

fn game_settings(ui: &mut Ui, dir: &mut String, lang: &mut String) -> Option<SettingsAction> {
    let mut action = None;
    ui.set_min_width(ui.available_width());
    section_title(ui, "Установка Dota 2");
    ui.label(muted(
        "Укажите корневую папку игры или найдите dota2.exe через «Обзор».",
    ));
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 100.).max(40.);
        let path = TextEdit::singleline(dir).hint_text(r"…\steamapps\common\dota 2 beta");
        ui.add_sized([width, 32.], path);
        if button(ui, "Обзор…", false).clicked() {
            action = Some(SettingsAction::Browse);
        }
    });
    ui.add_space(3.);
    ui.separator();
    ui.label(strong("Язык пакета", 15.));
    let selected = LANGUAGES
        .iter()
        .find(|(code, _)| *code == lang)
        .map_or("Выберите язык", |(_, label)| *label);
    ComboBox::from_id_salt("installation-language")
        .width(230.)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for &(code, label) in LANGUAGES {
                if ui.selectable_label(lang == code, label).clicked() {
                    *lang = code.to_owned();
                }
            }
        });
    let hint = format!(
        "Игра загрузит профиль из game/dota_{lang}. \
                        В Steam выберите тот же язык Dota 2 и дождитесь загрузки его файлов."
    );
    ui.label(muted(&hint));
    if button(ui, "Сохранить настройки", true).clicked() {
        action = Some(SettingsAction::Save);
    }
    action
}

fn installed_profile(ui: &mut Ui, installed: Option<&Installation>) -> bool {
    ui.set_min_width(ui.available_width());
    section_title(ui, "Профиль в игре");
    let Some(current) = installed else {
        ui.label(muted(
            "Профиль ещё не применён. Выберите ресурсы в библиотеке, \
                        примените профиль и перезапустите игру.",
        ));
        return false;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Установлен").color(MINT));
        ui.label(strong(&current.profile, 15.));
    });
    let path = current
        .root
        .join("game")
        .join(format!("dota_{}", current.language));
    ui.label(muted(&path.display().to_string()));
    ui.label(muted(
        "После изменения профиля примените его снова и перезапустите Dota 2.",
    ));
    button(ui, "Снять профиль", false).clicked()
}

fn model_help(ui: &mut Ui) {
    ui.set_min_width(ui.available_width());
    ui.label(strong("Свои модели и материалы", 16.));
    ui.label(muted(
        "Для исходной VMDL включите Dota 2 Workshop Tools в Steam и сохраните модель \
                    в проекте аддона. FBX сначала откройте в ModelDoc.",
    ));
    ui.label(muted(
        "Для модели с собственными материалами и текстурами включите зависимости \
                    по их игровым путям. Готовый VPK может содержать весь комплект.",
    ));
}

pub fn settings_page(
    ui: &mut Ui,
    dir: &mut String,
    lang: &mut String,
    volume: &mut u8,
    installed: Option<&Installation>,
) -> Option<SettingsAction> {
    ScrollArea::vertical()
        .id_salt("settings-page")
        .show(ui, |ui| {
            title(ui, "Настройки");
            ui.add_space(8.);
            box_frame().show(ui, |ui| {
                ui.label(strong("Звук БКБ", 16.));
                ui.add(Slider::new(volume, 0..=100).suffix(" %").text("Громкость"));
                if *volume == 0 {
                    ui.label(muted("Звук выключен"));
                }
            });
            ui.add_space(8.);
            let mut action = box_frame()
                .show(ui, |ui| game_settings(ui, dir, lang))
                .inner;
            ui.add_space(8.);
            if box_frame()
                .show(ui, |ui| installed_profile(ui, installed))
                .inner
            {
                action = Some(SettingsAction::Uninstall);
            }
            ui.add_space(8.);
            box_frame().show(ui, model_help);
            ui.add_space(8.);
            box_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(strong("Rift Atelier", 16.));
                ui.horizontal_wrapped(|ui| {
                    ui.hyperlink_to(
                        "Поддержать автора ↗",
                        "https://www.donationalerts.com/r/the_fpp",
                    );
                    ui.hyperlink_to("Исходный код ↗", env!("CARGO_PKG_REPOSITORY"));
                });
            });
            ui.add_space(12.);
            action
        })
        .inner
}
