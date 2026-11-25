use eframe::egui::{self, *};
pub const BG: Color32 = Color32::from_rgb(17, 18, 21);
pub const SIDE: Color32 = Color32::from_rgb(21, 22, 25);
pub const CARD: Color32 = Color32::from_rgb(27, 28, 32);
pub const LINE: Color32 = Color32::from_rgb(48, 49, 55);
pub const TEXT: Color32 = Color32::from_rgb(243, 240, 234);
pub const MUTED: Color32 = Color32::from_rgb(163, 164, 175);
pub const RED: Color32 = Color32::from_rgb(252, 98, 83);
pub const MINT: Color32 = Color32::from_rgb(136, 202, 174);
pub const LILAC: Color32 = Color32::from_rgb(188, 161, 236);
pub const GOLD: Color32 = Color32::from_rgb(226, 172, 113);
pub const BLUE: Color32 = Color32::from_rgb(128, 196, 213);
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("strong".into()))
}
pub fn serif(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}
pub fn strong(s: &str, size: f32) -> RichText {
    RichText::new(s).font(bold(size)).color(TEXT)
}
pub fn muted(s: &str) -> RichText {
    RichText::new(s).size(13.).color(MUTED)
}
pub fn install(ctx: &Context) {
    let mut defs = FontDefinitions::default();
    for (name, file, family) in [
        ("body", "segoeui.ttf", FontFamily::Proportional),
        ("strong", "seguisb.ttf", FontFamily::Name("strong".into())),
        (
            "display",
            "georgiab.ttf",
            FontFamily::Name("display".into()),
        ),
    ] {
        let mut fallback = defs.families[&FontFamily::Proportional].clone();
        let path = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:/Windows".into())
            .join("Fonts")
            .join(file);
        if let Ok(data) = std::fs::read(path) {
            defs.font_data
                .insert(name.into(), FontData::from_owned(data).into());
            fallback.insert(0, name.into());
        }
        defs.families.insert(family, fallback);
    }
    ctx.set_fonts(defs);
    let mut style = (*ctx.style()).clone();
    style.visuals = Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = CARD;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.faint_bg_color = SIDE;
    style.visuals.extreme_bg_color = Color32::from_rgb(19, 20, 23);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1_f32, LINE);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(41, 42, 48);
    style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(62, 61, 67);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(81, 57, 58);
    style.visuals.selection.bg_fill = Color32::from_rgb(113, 53, 53);
    style.visuals.hyperlink_color = RED;
    style.visuals.window_stroke = Stroke::new(1_f32, LINE);
    style.spacing.item_spacing = vec2(9., 8.);
    style.spacing.button_padding = vec2(11., 6.);
    style.spacing.interact_size.y = 29.;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(15.));
    style.text_styles.insert(TextStyle::Button, bold(14.));
    ctx.set_style(style);
}
pub fn button(ui: &mut Ui, s: &str, primary: bool) -> Response {
    ui.add(
        Button::new(RichText::new(s).font(bold(14.)).color(if primary {
            Color32::from_rgb(28, 16, 16)
        } else {
            TEXT
        }))
        .fill(if primary {
            RED
        } else {
            Color32::from_rgb(42, 43, 48)
        })
        .corner_radius(5.)
        .min_size(vec2(0., 35.)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}
pub fn box_frame() -> Frame {
    Frame::NONE
        .fill(CARD)
        .stroke(Stroke::new(1_f32, LINE))
        .corner_radius(7.)
        .inner_margin(16)
}
pub fn kicker(ui: &mut Ui, s: &str) {
    ui.label(RichText::new(s).font(mono(11.)).color(MUTED));
}
pub fn title(ui: &mut Ui, s: &str) {
    ui.label(RichText::new(s).font(serif(34.)).color(TEXT));
}
pub fn paint_icon(p: &Painter, center: Pos2, id: &str, size: f32, color: Color32) {
    let at = |x: f32, y: f32| center + vec2((x - 12.) * size / 24., (y - 12.) * size / 24.);
    let st = Stroke::new(1.5_f32, color);
    let poly = |v: &[(f32, f32)]| {
        p.add(Shape::line(v.iter().map(|(x, y)| at(*x, *y)).collect(), st));
    };
    match id {
        "home" => {
            for (x, y) in [(3., 3.), (14., 3.), (3., 14.), (14., 14.)] {
                p.rect_stroke(
                    Rect::from_min_max(at(x, y), at(x + 7., y + 7.)),
                    1.,
                    st,
                    StrokeKind::Inside,
                );
            }
        }
        "library" => {
            poly(&[(3., 5.), (6., 5.), (6., 20.), (3., 20.), (3., 5.)]);
            poly(&[(10., 4.), (13., 4.), (13., 20.), (10., 20.), (10., 4.)]);
            poly(&[(15., 6.), (18., 5.), (22., 19.), (19., 20.), (15., 6.)]);
        }
        "catalog" => {
            p.circle_stroke(center, size * 0.37, st);
            poly(&[(3., 12.), (21., 12.)]);
            poly(&[
                (12., 3.),
                (8., 8.),
                (8., 16.),
                (12., 21.),
                (16., 16.),
                (16., 8.),
                (12., 3.),
            ]);
        }
        "settings" => {
            for (y, x) in [(5., 8.), (12., 16.), (19., 10.)] {
                poly(&[(3., y), (x - 2., y)]);
                poly(&[(x + 2., y), (21., y)]);
                p.circle_stroke(at(x, y), size * 0.095, st);
            }
        }
        "profile" => {
            poly(&[
                (3., 6.),
                (9., 6.),
                (11., 8.),
                (21., 8.),
                (21., 19.),
                (3., 19.),
                (3., 6.),
            ]);
        }
        "arrow" => {
            poly(&[(6., 17.), (18., 5.)]);
            poly(&[(7., 5.), (18., 5.), (18., 16.)]);
        }
        "right" => {
            poly(&[(4., 12.), (20., 12.), (14., 6.)]);
            poly(&[(20., 12.), (14., 18.)]);
        }
        "stack" => {
            poly(&[(3., 9.), (12., 4.), (21., 9.), (12., 14.), (3., 9.)]);
            poly(&[(3., 13.), (12., 18.), (21., 13.)]);
            poly(&[(3., 17.), (12., 22.), (21., 17.)]);
        }
        "plus" => {
            poly(&[(4., 12.), (20., 12.)]);
            poly(&[(12., 4.), (12., 20.)]);
        }
        _ => {}
    }
}
pub fn mark(p: &Painter, r: Rect) {
    let at = |x: f32, y: f32| r.min + vec2(x * r.width(), y * r.height());
    for pts in [
        vec![(0.04, 0.9), (0.34, 0.12), (0.65, 0.12), (0.36, 0.53)],
        vec![
            (0.42, 0.56),
            (0.70, 0.16),
            (0.96, 0.12),
            (0.68, 0.58),
            (0.87, 0.9),
            (0.58, 0.9),
        ],
    ] {
        p.add(Shape::convex_polygon(
            pts.into_iter().map(|(x, y)| at(x, y)).collect(),
            RED,
            Stroke::NONE,
        ));
    }
}
pub fn nav(ui: &mut Ui, id: &str, title: &str, active: bool) -> bool {
    let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), 42.), Sense::click());
    let painter = ui.painter();
    if active || response.hovered() {
        painter.rect_filled(
            r,
            5.,
            if active {
                Color32::from_rgb(59, 35, 36)
            } else {
                CARD
            },
        );
    }
    if active {
        painter.rect_filled(
            Rect::from_min_size(r.left_center() - vec2(0., 8.), vec2(3., 16.)),
            2.,
            RED,
        );
    }
    let col = if active { RED } else { MUTED };
    paint_icon(painter, r.left_center() + vec2(21., 0.), id, 18., col);
    painter.text(
        r.left_center() + vec2(41., -1.),
        Align2::LEFT_CENTER,
        title,
        bold(14.),
        if active { TEXT } else { MUTED },
    );
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}
pub fn diagram(p: &Painter, r: Rect, kind: usize, col: Color32) {
    let p = p.with_clip_rect(r);
    let c = r.center();
    let s = r.height().min(r.width()) * 0.39;
    let pt = |x: f32, y: f32| c + vec2(x * s, y * s);
    let stroke = Stroke::new(1.15_f32, col);
    let curve = |v: Vec<Pos2>| {
        p.add(Shape::line(v, stroke));
    };
    let faint = col.gamma_multiply(0.18);
    for i in -4..=4 {
        let x = i as f32 * s * 0.48;
        p.line_segment(
            [c + vec2(x, -s * 1.26), c + vec2(x, s * 1.26)],
            Stroke::new(0.55_f32, faint),
        );
    }
    for i in -2..=2 {
        let y = i as f32 * s * 0.48;
        p.line_segment(
            [c + vec2(-s * 2., y), c + vec2(s * 2., y)],
            Stroke::new(0.55_f32, faint),
        );
    }
    match kind {
        0 => {
            // Wireframe swatch, warped over a curved surface.
            let surface = |u: f32, v: f32| {
                pt(
                    (u - v) * 0.73,
                    (u + v) * 0.36 + (u * 2.6 + v * 1.5).sin() * 0.23,
                )
            };
            for i in 0..=9 {
                let t = i as f32 / 4.5 - 1.;
                curve((0..=22).map(|j| surface(t, j as f32 / 11. - 1.)).collect());
                curve((0..=22).map(|j| surface(j as f32 / 11. - 1., t)).collect());
            }
        }
        1 => {
            // Faceted crystal, front and back edges.
            let a = pt(-0.08, -1.02);
            let b = pt(0.18, 1.02);
            let ring = [
                pt(-0.91, -0.12),
                pt(-0.46, 0.24),
                pt(0.28, 0.36),
                pt(0.94, 0.03),
                pt(0.52, -0.34),
                pt(-0.19, -0.46),
            ];
            for i in 0..ring.len() {
                let n = ring[(i + 1) % ring.len()];
                p.add(Shape::convex_polygon(
                    vec![a, ring[i], n],
                    col.gamma_multiply(0.04 + 0.026 * i as f32),
                    Stroke::NONE,
                ));
                curve(vec![a, ring[i], n, b]);
            }
        }
        _ => {
            for i in -16..=16 {
                let t = i as f32 / 16.;
                let height =
                    (1. - t.abs()).powf(0.66) * ((i as f32 * 1.73).cos().abs() * 0.8 + 0.19) + 0.04;
                let x = t * 1.45;
                p.line_segment([pt(x, -height), pt(x, height)], Stroke::new(2_f32, col));
            }
        }
    }
}
pub fn import_card(ui: &mut Ui, w: f32, kind: usize) -> bool {
    let (label, formats, col, bg) = [
        (
            "Текстура",
            "PNG · TGA · VTEX",
            LILAC,
            Color32::from_rgb(42, 35, 51),
        ),
        (
            "Модель",
            "VPK · ZIP · VMDL",
            GOLD,
            Color32::from_rgb(47, 38, 32),
        ),
        ("Звук", "WAV · VSND_C", BLUE, Color32::from_rgb(28, 42, 47)),
    ][kind];
    let (r, res) = ui.allocate_exact_size(vec2(w, 114.), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 6., bg);
    p.rect_stroke(
        r,
        6.,
        Stroke::new(
            1_f32,
            if res.hovered() {
                col
            } else {
                col.gamma_multiply(0.28)
            },
        ),
        StrokeKind::Inside,
    );
    let x = r.min.x + 15.;
    p.text(
        pos2(x, r.min.y + 16.),
        Align2::LEFT_TOP,
        label,
        bold(21.),
        TEXT,
    );
    p.text(
        pos2(x, r.max.y - 14.),
        Align2::LEFT_BOTTOM,
        formats,
        mono(10.5),
        col,
    );
    let dw = (w * 0.47).clamp(52., 134.);
    diagram(
        p,
        Rect::from_min_max(pos2(r.max.x - dw - 2., r.min.y + 28.), r.max - vec2(3., 5.)),
        kind,
        col,
    );
    paint_icon(p, pos2(r.max.x - 18., r.min.y + 19.), "arrow", 14., col);
    res.on_hover_cursor(CursorIcon::PointingHand).clicked()
}
pub fn image_data(im: image::DynamicImage) -> ColorImage {
    let im = im.to_rgba8();
    ColorImage::from_rgba_unmultiplied([im.width() as usize, im.height() as usize], im.as_raw())
}
pub fn icon_data() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("embedded application icon")
        .into_rgba8();
    egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}
