use crate::{
    store::{Item, State, fmt_size},
    theme::*,
};
use eframe::egui::*;
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Thumbnails(VecDeque<(PathBuf, Option<TextureHandle>)>);
impl Thumbnails {
    fn get(&mut self, ctx: &Context, file: &Path) -> Option<&TextureHandle> {
        if let Some(index) = self.0.iter().position(|(p, _)| p == file) {
            let entry = self.0.remove(index)?;
            self.0.push_front(entry);
        } else {
            // Covers produced by previews are normalized before they reach the UI.
            let texture = std::fs::metadata(file)
                .ok()
                .filter(|m| m.len() <= 4 * 1024 * 1024)
                .and_then(|_| image::ImageReader::open(file).ok())
                .and_then(|reader| {
                    let (w, h) = reader.into_dimensions().ok()?;
                    if w > 768 || h > 512 {
                        return None;
                    }
                    image::open(file).ok()
                })
                .map(|im| {
                    ctx.load_texture(
                        "library-cover",
                        image_data(im.thumbnail(124, 78)),
                        TextureOptions::LINEAR,
                    )
                });
            self.0.push_front((file.into(), texture));
            self.0.truncate(72);
        }
        self.0.front()?.1.as_ref()
    }
}

pub enum Action {
    Import,
    OpenLibrary,
    Toggle(String),
    Details(String),
    Delete(String),
    NewProfile,
    Select(usize),
    Move(usize, usize),
    Install,
    Export,
    Uninstall,
}

fn header(ui: &mut Ui, heading: &str, command: &str, primary: bool) -> bool {
    ui.horizontal(|ui| {
        title(ui, heading);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            button(ui, command, primary).clicked()
        })
        .inner
    })
    .inner
}

fn control(ui: &mut Ui, rect: Rect, id: &str, label: &str, enabled: bool) -> Response {
    let res = ui.interact(
        rect,
        ui.id().with(id),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    res.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if enabled {
        res.on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(label)
    } else {
        res
    }
}

fn row_icon(ui: &Ui, rect: Rect, kind: &str) {
    let color = match kind {
        "Модель" => GOLD,
        "Звук" => BLUE,
        "Пакет" => MINT,
        _ => LILAC,
    };
    let p = ui.painter();
    p.rect_filled(rect, 6., color.gamma_multiply(0.13));
    let c = rect.center();
    let pts = |points: &[(f32, f32)]| {
        p.add(Shape::line(
            points.iter().map(|(x, y)| c + vec2(*x, *y)).collect(),
            Stroke::new(1.4_f32, color),
        ));
    };
    match kind {
        "Звук" => {
            for (x, h) in [(-8., 2.), (-4., 6.), (0., 9.), (4., 4.), (8., 7.)] {
                pts(&[(x, -h), (x, h)]);
            }
        }
        "Модель" => {
            pts(&[
                (0., -9.),
                (8., -4.),
                (8., 5.),
                (0., 10.),
                (-8., 5.),
                (-8., -4.),
                (0., -9.),
            ]);
            pts(&[(-8., -4.), (0., 1.), (8., -4.)]);
            pts(&[(0., 1.), (0., 10.)]);
        }
        "Пакет" => paint_icon(p, c, "stack", 22., color),
        _ => {
            pts(&[(-8., -7.), (8., -7.), (8., 7.), (-8., 7.), (-8., -7.)]);
            pts(&[(-8., 4.), (-3., -1.), (0., 2.), (3., -2.), (8., 4.)]);
            p.circle_filled(c + vec2(-3., -4.), 1.25, color);
        }
    }
}

fn empty_panel(ui: &mut Ui, icon: &str, heading: &str, detail: &str, link: Option<&str>) -> bool {
    box_frame()
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(13.);
                let (tile, _) = ui.allocate_exact_size(vec2(42., 42.), Sense::hover());
                ui.painter()
                    .rect_filled(tile, 10., Color32::from_rgb(53, 38, 39));
                paint_icon(ui.painter(), tile.center(), icon, 23., RED);
                ui.add_space(3.);
                ui.label(strong(heading, 18.));
                ui.label(muted(detail));
                let clicked = link.is_some_and(|s| button(ui, s, false).clicked());
                ui.add_space(12.);
                clicked
            })
            .inner
        })
        .inner
}

fn count_label(ui: &mut Ui, caption: &str, n: usize) {
    ui.horizontal(|ui| {
        kicker(ui, caption);
        ui.label(RichText::new(format!("{n:02}")).font(mono(11.)).color(TEXT));
    });
}

fn library_row(
    ui: &mut Ui,
    item: &Item,
    in_profile: bool,
    thumbs: &mut Thumbnails,
) -> Option<Action> {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 60.), Sense::hover());
    let cy = r.center().y;
    let delete = Rect::from_center_size(pos2(r.right() - 22., cy), vec2(28., 30.));
    let toggle = Rect::from_min_size(pos2(delete.left() - 110., cy - 14.), vec2(100., 28.));
    let body = Rect::from_min_max(r.min, pos2(toggle.left() - 8., r.max.y));
    let details = control(ui, body, "details", "Открыть ресурс", true);
    let switch = control(
        ui,
        toggle,
        "toggle",
        if in_profile {
            "Убрать из профиля"
        } else {
            "Добавить в профиль"
        },
        true,
    );
    let remove = control(ui, delete, "delete", "Удалить из библиотеки", true);
    let p = ui.painter();
    p.rect_filled(
        r,
        5.,
        if details.hovered() {
            Color32::from_rgb(34, 35, 40)
        } else {
            CARD
        },
    );
    let icon = Rect::from_center_size(pos2(r.left() + 42., cy), vec2(66., 44.));
    if let Some(tex) = item
        .cover
        .as_ref()
        .and_then(|(path, _)| thumbs.get(ui.ctx(), path))
    {
        let s = tex.size_vec2();
        let ratio = (icon.width() / s.x).min(icon.height() / s.y);
        p.rect_filled(icon, 4., Color32::from_gray(21));
        p.image(
            tex.id(),
            Rect::from_center_size(icon.center(), s * ratio),
            Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
            Color32::WHITE,
        );
    } else {
        row_icon(ui, icon, &item.kind);
    }
    let text_x = icon.right() + 12.;
    let clipped = p.with_clip_rect(Rect::from_min_max(pos2(text_x, r.top()), body.max));
    clipped.text(
        pos2(text_x, cy - 9.),
        Align2::LEFT_CENTER,
        &item.name,
        bold(15.),
        TEXT,
    );
    let second = if item.target.is_empty() {
        format!(
            "{}   ·   {}   ·   Файлов: {}",
            item.kind,
            fmt_size(item.size),
            item.count
        )
    } else {
        format!(
            "{}   ·   {}   ·   {}",
            item.kind,
            fmt_size(item.size),
            item.target
        )
    };
    clipped.text(
        pos2(text_x, cy + 11.),
        Align2::LEFT_CENTER,
        second,
        FontId::proportional(11.5),
        MUTED,
    );
    let fill = if in_profile {
        Color32::from_rgb(38, 55, 49)
    } else {
        Color32::from_rgb(43, 44, 49)
    };
    let ink = if in_profile { MINT } else { MUTED };
    p.rect_filled(toggle, 5., fill);
    if switch.hovered() {
        p.rect_stroke(toggle, 5., Stroke::new(1.0_f32, ink), StrokeKind::Inside);
    }
    if in_profile {
        let a = toggle.left_center() + vec2(12., 0.);
        p.add(Shape::line(
            vec![a - vec2(2., 0.), a + vec2(1., 3.), a + vec2(6., -3.)],
            Stroke::new(1.6_f32, ink),
        ));
    } else {
        paint_icon(p, toggle.left_center() + vec2(15., 0.), "plus", 13., ink);
    }
    p.text(
        toggle.left_center() + vec2(26., 0.),
        Align2::LEFT_CENTER,
        if in_profile {
            "Выбрано"
        } else {
            "Выбрать"
        },
        bold(12.),
        ink,
    );
    if remove.hovered() {
        p.rect_filled(delete, 4., RED.gamma_multiply(0.12));
    }
    let c = delete.center();
    let s = Stroke::new(1.3_f32, if remove.hovered() { RED } else { MUTED });
    p.line_segment([c + vec2(-6., -4.), c + vec2(6., -4.)], s);
    p.line_segment([c + vec2(-2.5, -6.5), c + vec2(2.5, -6.5)], s);
    p.add(Shape::line(
        vec![
            c + vec2(-4.5, -2.),
            c + vec2(-3.5, 5.5),
            c + vec2(3.5, 5.5),
            c + vec2(4.5, -2.),
        ],
        s,
    ));
    if details.clicked() {
        Some(Action::Details(item.id.clone()))
    } else if switch.clicked() {
        Some(Action::Toggle(item.id.clone()))
    } else if remove.clicked() {
        Some(Action::Delete(item.id.clone()))
    } else {
        None
    }
}

pub const SEARCH_FIELD: &str = "library-search";

#[derive(Default)]
pub struct LibraryView {
    search: String,
    kind: String,
    selected_only: bool,
    order: LibraryOrder,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum LibraryOrder {
    #[default]
    Recent,
    Name,
    Size,
}

impl LibraryOrder {
    fn label(self) -> &'static str {
        match self {
            Self::Recent => "Сначала новые",
            Self::Name => "По названию",
            Self::Size => "Сначала крупные",
        }
    }
}

impl LibraryView {
    fn clear_filters(&mut self) {
        self.search.clear();
        self.kind.clear();
        self.selected_only = false;
    }

    fn rows<'a>(&self, state: &'a State) -> Vec<(&'a Item, bool)> {
        let selected: HashSet<&str> = state
            .profiles
            .get(state.selected)
            .into_iter()
            .flat_map(|p| p.ids.iter().map(String::as_str))
            .collect();
        let query = self.search.to_lowercase();
        let words: Vec<_> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect();
        let mut rows: Vec<_> = state
            .items
            .iter()
            .rev()
            .filter_map(|item| {
                let active = selected.contains(item.id.as_str());
                if (self.selected_only && !active)
                    || (!self.kind.is_empty() && self.kind != item.kind)
                {
                    return None;
                }
                if !words.is_empty() {
                    let text =
                        format!("{} {} {}", item.name, item.target, item.kind).to_lowercase();
                    if !words.iter().all(|w| text.contains(w)) {
                        return None;
                    }
                }
                Some((item, active))
            })
            .collect();
        match self.order {
            LibraryOrder::Recent => {}
            LibraryOrder::Name => rows.sort_by_cached_key(|(i, _)| i.name.to_lowercase()),
            LibraryOrder::Size => rows.sort_by_key(|(i, _)| std::cmp::Reverse(i.size)),
        }
        rows
    }
}

pub fn library_page(
    ui: &mut Ui,
    state: &State,
    view: &mut LibraryView,
    thumbs: &mut Thumbnails,
) -> Option<Action> {
    let mut action = header(ui, "Библиотека", "+  Импортировать", true).then_some(Action::Import);
    ui.add_space(9.);
    Frame::NONE
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(6.)
        .inner_margin(Margin::symmetric(12, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16., 18.), Sense::hover());
                let c = r.center() - vec2(2., 2.);
                ui.painter()
                    .circle_stroke(c, 4.8, Stroke::new(1.4_f32, MUTED));
                ui.painter().line_segment(
                    [c + vec2(3.2, 3.2), c + vec2(8., 8.)],
                    Stroke::new(1.4_f32, MUTED),
                );
                let width = (ui.available_width() - 30.).max(64.);
                let field = ui.add(
                    TextEdit::singleline(&mut view.search)
                        .id(Id::new(SEARCH_FIELD))
                        .hint_text("Название, тип или игровой путь   ·   Ctrl+F")
                        .frame(false)
                        .desired_width(width),
                );
                if field.gained_focus() {
                    field.scroll_to_me(Some(Align::Center));
                }
                if !view.search.is_empty()
                    && ui
                        .small_button("×")
                        .on_hover_text("Очистить поиск")
                        .clicked()
                {
                    view.search.clear();
                    ui.memory_mut(|m| m.request_focus(Id::new(SEARCH_FIELD)));
                }
            });
        });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 5.;
        for (label, value) in [
            ("Все", ""),
            ("Текстуры", "Текстура"),
            ("Модели", "Модель"),
            ("Звуки", "Звук"),
            ("Материалы", "Материал"),
            ("Пакеты", "Пакет"),
        ] {
            let selected = view.kind == value;
            let tab = Button::new(RichText::new(label).font(bold(12.)).color(if selected {
                RED
            } else {
                MUTED
            }))
            .fill(if selected {
                Color32::from_rgb(56, 36, 38)
            } else {
                SIDE
            })
            .corner_radius(4.);
            if ui
                .add(tab)
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                view.kind = value.into();
            }
        }
    });
    ui.add_space(5.);
    if state.items.is_empty() {
        if empty_panel(
            ui,
            "library",
            "Библиотека пуста",
            "Перетащите файл в окно или выберите его на диске.",
            Some("Импортировать файл"),
        ) {
            action = Some(Action::Import);
        }
        return action;
    }
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut view.selected_only, "Только выбранные")
            .on_hover_text("Показать ресурсы текущего профиля");
        ui.separator();
        ComboBox::from_id_salt("library_order")
            .width(148.)
            .selected_text(view.order.label())
            .show_ui(ui, |ui| {
                for order in [LibraryOrder::Recent, LibraryOrder::Name, LibraryOrder::Size] {
                    ui.selectable_value(&mut view.order, order, order.label());
                }
            })
            .response
            .on_hover_text("Сортировка библиотеки");
    });
    let rows = view.rows(state);
    if rows.is_empty() {
        if empty_panel(
            ui,
            "library",
            "Ресурсы не найдены",
            "Попробуйте другое название или сбросьте фильтры.",
            Some("Сбросить фильтры"),
        ) {
            view.clear_filters();
        }
    } else {
        count_label(ui, "РЕСУРСЫ", rows.len());
        for (i, on) in rows {
            if !ui.is_rect_visible(Rect::from_min_size(
                ui.next_widget_position(),
                vec2(ui.available_width(), 60.),
            )) {
                ui.allocate_space(vec2(ui.available_width(), 60.));
                continue;
            }
            if let Some(selected) = ui.push_id(&i.id, |ui| library_row(ui, i, on, thumbs)).inner {
                action = Some(selected);
            }
        }
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, name: &str, kind: &str, target: &str, size: u64) -> Item {
        Item {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            target: target.into(),
            file: PathBuf::new(),
            preview: None,
            cover: None,
            size,
            count: 1,
        }
    }

    fn ids<'a>(view: &LibraryView, state: &'a State) -> Vec<&'a str> {
        view.rows(state)
            .into_iter()
            .map(|(item, _)| item.id.as_str())
            .collect()
    }

    #[test]
    fn library_combines_search_words_type_and_current_profile() {
        let mut state = State::default();
        state.items = vec![
            item(
                "blue",
                "Queen.Of.Pain Синий",
                "Модель",
                "models/heroes/queenofpain/wings.vmdl_c",
                50,
            ),
            item(
                "red",
                "Queen_Of_Pain Красный",
                "Модель",
                "models/heroes/queenofpain/wings.vmdl_c",
                80,
            ),
            item(
                "tex",
                "Queen Of Pain",
                "Текстура",
                "models/heroes/queenofpain/wings.vtex_c",
                20,
            ),
            item(
                "other",
                "Queen Of Pain",
                "Модель",
                "models/heroes/queenofpain/crown.vmdl_c",
                30,
            ),
        ];
        state.profiles[0].ids = vec!["blue".into(), "tex".into(), "other".into()];
        let mut view = LibraryView {
            search: r"PAIN  models\heroes WINGS".into(),
            kind: "Модель".into(),
            selected_only: true,
            ..Default::default()
        };
        assert_eq!(ids(&view, &state), ["blue"]);
        view.selected_only = false;
        assert_eq!(ids(&view, &state), ["red", "blue"]);
        view.search = "синий   queen of pain".into();
        assert_eq!(ids(&view, &state), ["blue"]);
        view.search = "missing".into();
        assert!(view.rows(&state).is_empty());
        view.clear_filters();
        assert_eq!(ids(&view, &state), ["other", "tex", "red", "blue"]);
    }

    #[test]
    fn library_sort_changes_only_the_view_and_keeps_equal_rows_recent_first() {
        let mut state = State::default();
        state.items = vec![
            item("old", "alpha", "Звук", "", 5),
            item("mid", "Bravo", "Звук", "", 8),
            item("new", "ALPHA", "Звук", "", 5),
        ];
        state.profiles[0].ids = vec!["old".into(), "new".into()];
        let mut view = LibraryView::default();
        assert_eq!(ids(&view, &state), ["new", "mid", "old"]);
        view.order = LibraryOrder::Name;
        assert_eq!(ids(&view, &state), ["new", "old", "mid"]);
        view.order = LibraryOrder::Size;
        assert_eq!(ids(&view, &state), ["mid", "new", "old"]);
        assert_eq!(state.profiles[0].ids, ["old", "new"]);
        assert_eq!(state.items[0].id, "old");
    }
}

fn order_button(ui: &mut Ui, r: Rect, id: &str, hint: &str, enabled: bool) -> bool {
    let response = control(ui, r, id, hint, enabled);
    let p = ui.painter();
    if enabled && response.hovered() {
        p.rect_filled(
            r,
            4.,
            if id == "remove" {
                RED.gamma_multiply(0.12)
            } else {
                LINE
            },
        );
    }
    let color = if !enabled {
        LINE
    } else if id == "remove" && response.hovered() {
        RED
    } else {
        MUTED
    };
    let c = r.center();
    let st = Stroke::new(1.5_f32, color);
    if id == "remove" {
        p.line_segment([c - vec2(4., 4.), c + vec2(4., 4.)], st);
        p.line_segment([c + vec2(-4., 4.), c + vec2(4., -4.)], st);
    } else {
        let sign = if id == "up" { -1. } else { 1. };
        p.add(Shape::line(
            vec![
                c + vec2(-4., -2. * sign),
                c + vec2(0., 2. * sign),
                c + vec2(4., -2. * sign),
            ],
            st,
        ));
    }
    enabled && response.clicked()
}

fn order_row(
    ui: &mut Ui,
    id: &str,
    item: Option<&Item>,
    at: usize,
    count: usize,
) -> Option<Action> {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 47.), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 4., CARD);
    let cy = r.center().y;
    p.text(
        pos2(r.left() + 12., cy),
        Align2::LEFT_CENTER,
        format!("{:02}", at + 1),
        mono(12.),
        MUTED,
    );
    let text_rect = Rect::from_min_max(r.min + vec2(47., 0.), r.max - vec2(110., 0.));
    p.with_clip_rect(text_rect).text(
        text_rect.left_center(),
        Align2::LEFT_CENTER,
        item.map(|i| i.name.as_str()).unwrap_or("Файл отсутствует"),
        bold(14.),
        TEXT,
    );
    let slot = |x: f32| Rect::from_center_size(pos2(r.right() - x, cy), vec2(29., 30.));
    if order_button(ui, slot(90.), "up", "Переместить выше", at > 0) {
        return Some(Action::Move(at, at - 1));
    }
    if order_button(ui, slot(56.), "down", "Переместить ниже", at + 1 < count) {
        return Some(Action::Move(at, at + 1));
    }
    if order_button(ui, slot(22.), "remove", "Убрать из профиля", true) {
        return Some(Action::Toggle(id.into()));
    }
    None
}

pub fn installation_bar(ui: &mut Ui, state: &State) -> Option<Action> {
    let profile = state.profiles.get(state.selected)?;
    if profile.ids.is_empty() && state.installed.is_none() {
        return None;
    }
    let applied = state.current_is_installed();
    let (caption, hint, color) = match (&state.installed, applied) {
        (_, true) => (
            "Профиль записан в Dota 2",
            "Игра читает замены при запуске.",
            MINT,
        ),
        (None, false) => (
            "Профиль ещё не установлен",
            "Примените выбранные моды, затем запустите игру.",
            GOLD,
        ),
        (Some(_), false) if profile.ids.is_empty() => (
            "Выбор пуст, в игре есть моды",
            "Можно снять ранее установленный пакет.",
            GOLD,
        ),
        (Some(old), false) if old.ids.is_none() => (
            "Требуется проверка установки",
            "Примените профиль для записи и проверки файлов.",
            GOLD,
        ),
        (Some(_), false) => (
            "Изменения ещё не применены",
            "В папке игры находится предыдущая сборка.",
            GOLD,
        ),
    };
    let mut action = None;
    box_frame()
        .inner_margin(Margin::symmetric(14, 9))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(caption).font(bold(15.)).color(color));
                        ui.label(muted(&format!("·  Выбрано: {}", profile.ids.len())));
                    });
                    ui.label(muted(hint));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if profile.ids.is_empty() {
                        if button(ui, "Снять установку", false).clicked() {
                            action = Some(Action::Uninstall);
                        }
                    } else if button(
                        ui,
                        if applied {
                            "Применить повторно"
                        } else {
                            "Применить в Dota 2"
                        },
                        !applied,
                    )
                    .clicked()
                    {
                        action = Some(Action::Install);
                    }
                });
            });
        });
    ui.add_space(4.);
    action
}

pub fn profiles_page(ui: &mut Ui, state: &State) -> Option<Action> {
    let mut action = header(ui, "Профили", "+  Новый профиль", false).then_some(Action::NewProfile);
    ui.add_space(9.);
    ui.horizontal_wrapped(|ui| {
        for (index, profile) in state.profiles.iter().enumerate() {
            let on = index == state.selected;
            let width = (ui
                .painter()
                .layout_no_wrap(profile.name.clone(), bold(13.), TEXT)
                .size()
                .x
                + 43.)
                .clamp(86., 205.);
            let (r, res) = ui.allocate_exact_size(vec2(width, 33.), Sense::click());
            let p = ui.painter();
            p.rect_filled(
                r,
                5.,
                if on {
                    Color32::from_rgb(57, 38, 39)
                } else {
                    CARD
                },
            );
            if on || res.hovered() {
                p.rect_stroke(
                    r,
                    5.,
                    Stroke::new(1.0_f32, if on { RED.gamma_multiply(0.55) } else { MUTED }),
                    StrokeKind::Inside,
                );
            }
            paint_icon(
                p,
                r.left_center() + vec2(17., 0.),
                "profile",
                16.,
                if on { RED } else { MUTED },
            );
            let tr = Rect::from_min_max(r.min + vec2(32., 0.), r.max - vec2(8., 0.));
            p.with_clip_rect(tr).text(
                tr.left_center(),
                Align2::LEFT_CENTER,
                &profile.name,
                bold(13.),
                if on { TEXT } else { MUTED },
            );
            res.widget_info(|| {
                WidgetInfo::selected(WidgetType::SelectableLabel, true, on, &profile.name)
            });
            if res
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(&profile.name)
                .clicked()
            {
                action = Some(Action::Select(index));
            }
        }
    });
    let profile = state.profiles.get(state.selected)?;
    ui.add_space(8.);
    box_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(&profile.name).font(serif(23.)).color(TEXT));
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!profile.ids.is_empty(), |ui| {
                ui.horizontal(|ui| {
                    if button(ui, "Экспорт VPK", false).clicked() {
                        action = Some(Action::Export)
                    }
                });
            });
        });
        if let Some(applied) = &state.installed {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Последняя установка: «{}»", applied.profile))
                        .size(13.)
                        .color(MINT),
                );
                if ui.link("Снять замены").clicked() {
                    action = Some(Action::Uninstall);
                }
            });
        }
    });
    ui.add_space(5.);
    count_label(ui, "ПОРЯДОК ЗАМЕН", profile.ids.len());
    if profile.ids.is_empty() {
        if empty_panel(
            ui,
            "stack",
            "В профиле пока нет замен",
            "Выберите в библиотеке ресурсы для этого профиля.",
            Some("Открыть библиотеку"),
        ) {
            action = Some(Action::OpenLibrary);
        }
    } else {
        ui.label(muted(
            "Если два мода заменяют один файл, используется тот, что ниже в списке.",
        ));
        for (n, id) in profile.ids.iter().enumerate() {
            let item = state.items.iter().find(|i| &i.id == id);
            if let Some(selected) = ui
                .push_id(("order", n), |ui| {
                    order_row(ui, id, item, n, profile.ids.len())
                })
                .inner
            {
                action = Some(selected);
            }
        }
    }
    action
}
