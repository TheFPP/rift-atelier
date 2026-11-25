use crate::{store::State, theme::*};
use eframe::egui::*;
pub enum Action {
    Browse(usize),
    Library,
    Profiles,
    Detail(String),
    Catalog,
}
pub fn show(
    ui: &mut Ui,
    showcase: &mut crate::showcase::Showcase,
    state: &State,
) -> Option<Action> {
    let w = ui.available_width();
    let mut action = match showcase.show(ui) {
        Some(crate::showcase::Choice::Browse) => Some(Action::Catalog),
        Some(crate::showcase::Choice::Import) => Some(Action::Browse(3)),
        None => None,
    };
    ui.add_space(13.);
    ui.horizontal(|ui| {
        ui.label(strong("Добавить свой ресурс", 22.));
    });
    ui.add_space(3.);
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let cw = (ui.available_width() - 2. * gap) / 3.;
        for k in 0..3 {
            if import_card(ui, cw, k) {
                action = Some(Action::Browse(k));
            }
        }
    });
    ui.add_space(14.);
    ui.horizontal(|ui| {
        ui.label(strong("Моя библиотека", 22.));
        ui.label(
            RichText::new(format!("{:02}", state.items.len()))
                .font(mono(12.))
                .color(MUTED),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .link(RichText::new("Открыть библиотеку  →").size(13.))
                .clicked()
            {
                action = Some(Action::Library);
            }
        });
    });
    ui.add_space(2.);
    if state.items.is_empty() {
        let (empty, _) = ui.allocate_exact_size(vec2(w, 70.), Sense::hover());
        let p = ui.painter();
        p.rect_filled(empty, 6., CARD);
        p.rect_stroke(empty, 6., Stroke::new(1_f32, LINE), StrokeKind::Inside);
        p.rect_filled(
            Rect::from_center_size(empty.left_center() + vec2(36., 0.), vec2(39., 39.)),
            6.,
            Color32::from_rgb(40, 41, 46),
        );
        paint_icon(p, empty.left_center() + vec2(36., 0.), "stack", 21., MUTED);
        p.text(
            empty.left_center() + vec2(69., -9.),
            Align2::LEFT_CENTER,
            "Библиотека пока пуста",
            bold(15.),
            TEXT,
        );
        p.text(
            empty.left_center() + vec2(69., 12.),
            Align2::LEFT_CENTER,
            "Перетащите файл сюда или выберите ресурс выше.",
            FontId::proportional(13.),
            MUTED,
        );
    } else {
        for item in state.items.iter().rev().take(3) {
            let row = box_frame()
                .inner_margin(Margin::symmetric(13, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(strong(&item.name, 15.));
                        ui.label(muted(&item.kind));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(muted(&crate::store::fmt_size(item.size)))
                        });
                    });
                })
                .response;
            if row
                .interact(Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                action = Some(Action::Detail(item.id.clone()));
            }
        }
    }
    ui.add_space(8.);
    ui.horizontal_wrapped(|ui| {
        if ui.link("Открыть каталог Dota2PornFx  →").clicked() {
            action = Some(Action::Catalog);
        }
        ui.separator();
        if ui.link("Перейти к сборке  →").clicked() {
            action = Some(Action::Profiles);
        }
    });
    action
}
