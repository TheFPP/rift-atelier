mod portraits;
use crate::theme::*;
use eframe::egui::*;
use std::{path::PathBuf, sync::mpsc, thread};
struct Hero {
    name: &'static str,
    art: &'static str,
    glow: Color32,
    light: Color32,
}
const HEROES: [Hero; 6] = [
    Hero {
        name: "Outworld Destroyer",
        art: "obsidian_destroyer",
        glow: Color32::from_rgb(43, 70, 71),
        light: Color32::from_rgb(111, 228, 195),
    },
    Hero {
        name: "Dawnbreaker",
        art: "dawnbreaker",
        glow: Color32::from_rgb(86, 62, 37),
        light: Color32::from_rgb(250, 197, 113),
    },
    Hero {
        name: "Queen of Pain",
        art: "queenofpain",
        glow: Color32::from_rgb(67, 48, 71),
        light: Color32::from_rgb(226, 155, 208),
    },
    Hero {
        name: "Juggernaut",
        art: "juggernaut",
        glow: Color32::from_rgb(86, 50, 36),
        light: Color32::from_rgb(244, 147, 87),
    },
    Hero {
        name: "Lifestealer",
        art: "life_stealer",
        glow: Color32::from_rgb(83, 40, 35),
        light: Color32::from_rgb(246, 121, 92),
    },
    Hero {
        name: "Brewmaster",
        art: "brewmaster",
        glow: Color32::from_rgb(47, 66, 53),
        light: Color32::from_rgb(164, 196, 134),
    },
];
pub enum Choice {
    Browse,
    Import,
}
pub struct Showcase {
    index: usize,
    texture: Option<TextureHandle>,
    pending: Option<mpsc::Receiver<(usize, Option<image::DynamicImage>)>>,
    cache: PathBuf,
    failed: bool,
}
impl Showcase {
    pub fn new(cache: PathBuf) -> Self {
        Self {
            index: 0,
            texture: None,
            pending: None,
            cache,
            failed: false,
        }
    }
    fn portrait(&mut self, ctx: &Context) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok((index, image)) => {
                    self.pending = None;
                    if index == self.index {
                        self.texture = image.map(|im| {
                            ctx.load_texture(
                                HEROES[index].name,
                                image_data(im),
                                TextureOptions::LINEAR,
                            )
                        });
                        self.failed = self.texture.is_none();
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.failed = true;
                }
            }
        }
        if self.texture.is_none() && !self.failed {
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            let cache = self.cache.clone();
            let index = self.index;
            let ctx = ctx.clone();
            thread::spawn(move || {
                let _ = tx.send((index, portraits::load(&cache, HEROES[index].art)));
                ctx.request_repaint();
            });
        }
    }
    pub fn show(&mut self, ui: &mut Ui) -> Option<Choice> {
        self.portrait(ui.ctx());
        let hero = &HEROES[self.index];
        let w = ui.available_width();
        let h = (w * 0.295).clamp(218., 290.);
        let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let p = ui.painter_at(r);
        let left = Color32::from_rgb(27, 29, 32);
        let right = Color32::from_rgb(35, 34, 34);
        p.rect_filled(r, 7., left);
        let inset = r.shrink(1.);
        let mut m = Mesh::default();
        for (fraction, col) in [(0., left), (0.32, left), (0.70, hero.glow), (1., right)] {
            let x = inset.left() + fraction * inset.width();
            m.colored_vertex(pos2(x, inset.top()), col);
            m.colored_vertex(pos2(x, inset.bottom()), col);
        }
        for i in [0, 2, 4] {
            m.add_triangle(i, i + 1, i + 2);
            m.add_triangle(i + 1, i + 3, i + 2);
        }
        p.add(Shape::mesh(m));
        let center = pos2(r.left() + w * 0.73, r.top() + h * 0.53);
        let rim = hero.light.gamma_multiply(0.32);
        for k in [0.41, 0.48] {
            p.circle_stroke(center, h * k, Stroke::new(1_f32, rim));
        }
        for i in 0..60 {
            let a = (i as f32 / 60.) * std::f32::consts::TAU;
            let d = vec2(a.cos(), a.sin());
            p.line_segment(
                [
                    center + d * h * 0.48,
                    center + d * (h * 0.48 - if i % 5 == 0 { 7. } else { 3. }),
                ],
                Stroke::new(1_f32, rim),
            );
        }
        if let Some(texture) = &self.texture {
            let s = texture.size_vec2();
            let sh = h * 1.45;
            let rect =
                Rect::from_center_size(center + vec2(0., h * 0.17), vec2(sh * s.x / s.y, sh));
            p.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
                Color32::WHITE,
            );
        } else if self.failed {
            let retry = Rect::from_center_size(center, vec2(142., 34.));
            p.rect_filled(retry, 5., CARD);
            p.text(
                center,
                Align2::CENTER_CENTER,
                "Загрузить портрет",
                bold(13.),
                hero.light,
            );
            if ui
                .interact(retry, ui.id().with("portrait_retry"), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                self.failed = false;
                ui.ctx().request_repaint();
            }
        } else {
            p.text(
                center,
                Align2::CENTER_CENTER,
                "Загрузка портрета…",
                bold(13.),
                hero.light,
            );
        }
        let x = r.left() + 24.;
        let y = r.top() + 22.;
        let mut selected = self.index;
        p.line_segment(
            [pos2(x, y + 5.), pos2(x + 19., y + 5.)],
            Stroke::new(2_f32, hero.light),
        );
        let selector = Rect::from_min_size(pos2(x + 26., y - 9.), vec2(155., 30.));
        let mut selector_ui = ui.new_child(UiBuilder::new().max_rect(selector));
        ComboBox::from_id_salt("showcase_hero")
            .width(selector.width())
            .height(290.)
            .selected_text(
                RichText::new("Выбрать героя")
                    .font(bold(12.))
                    .color(hero.light),
            )
            .show_ui(&mut selector_ui, |ui| {
                for (index, candidate) in HEROES.iter().enumerate() {
                    ui.selectable_value(&mut selected, index, candidate.name);
                }
            });
        let preferred = if w > 700. { 46. } else { 38. };
        let title = p.layout_no_wrap(hero.name.into(), serif(preferred), TEXT);
        let size = preferred * ((w * 0.52 - 18.) / title.size().x.max(1.)).min(1.);
        p.text(
            pos2(x - 2., y + 22.),
            Align2::LEFT_TOP,
            hero.name,
            serif(size),
            TEXT,
        );
        p.text(
            pos2(x, y + size + 34.),
            Align2::LEFT_TOP,
            "Исходная модель Dota 2",
            FontId::proportional(15.),
            MUTED,
        );
        let mut action = None;
        let primary = Rect::from_min_size(pos2(x, r.bottom() - 58.), vec2(154., 34.));
        if card_button(ui, &p, primary, "catalog", "Каталог модов", true) {
            action = Some(Choice::Browse);
        }
        let secondary = Rect::from_min_size(primary.right_top() + vec2(8., 0.), vec2(149., 34.));
        if card_button(ui, &p, secondary, "import", "Добавить файл", false) {
            action = Some(Choice::Import);
        }
        let bx = r.right() - 17.;
        let by = r.bottom() - 19.;
        for (delta, id, label, xx) in [(-1i32, "prev", "‹", bx - 56.), (1, "next", "›", bx - 20.)]
        {
            let square = Rect::from_center_size(pos2(xx, by), vec2(31., 25.));
            let res = ui
                .interact(square, ui.id().with(id), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            p.rect_filled(
                square,
                4.,
                Color32::from_black_alpha(if res.hovered() { 220 } else { 166 }),
            );
            p.text(
                square.center() - vec2(0., 2.),
                Align2::CENTER_CENTER,
                label,
                bold(20.),
                TEXT,
            );
            if res.clicked() {
                selected = (self.index as i32 + delta).rem_euclid(HEROES.len() as i32) as usize;
            }
        }
        p.rect_stroke(r, 7., Stroke::new(1_f32, LINE), StrokeKind::Inside);
        if selected != self.index {
            self.index = selected;
            self.texture = None;
            self.failed = false;
            ui.ctx().request_repaint();
        }
        action
    }
}
fn card_button(ui: &Ui, p: &Painter, r: Rect, id: &str, text: &str, primary: bool) -> bool {
    let res = ui
        .interact(r, ui.id().with(id), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let fill = if primary {
        if res.hovered() {
            Color32::from_rgb(255, 127, 105)
        } else {
            RED
        }
    } else if res.hovered() {
        Color32::from_rgb(63, 60, 58)
    } else {
        Color32::from_rgb(45, 43, 44)
    };
    p.rect_filled(r, 4., fill);
    if !primary {
        p.rect_stroke(
            r,
            4.,
            Stroke::new(1_f32, Color32::from_gray(72)),
            StrokeKind::Inside,
        );
    }
    let ink = if primary {
        Color32::from_rgb(26, 18, 17)
    } else {
        TEXT
    };
    paint_icon(
        p,
        r.left_center() + vec2(18., 0.),
        if primary { "catalog" } else { "plus" },
        15.,
        ink,
    );
    p.text(
        r.left_center() + vec2(33., -1.),
        Align2::LEFT_CENTER,
        text,
        bold(13.5),
        ink,
    );
    res.clicked()
}
