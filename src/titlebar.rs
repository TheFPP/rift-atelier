use crate::theme::*;
use eframe::egui::*;

pub fn show(ctx: &Context, gold: bool) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let accent = if gold { GOLD } else { RED };
    TopBottomPanel::top("window-title")
        .exact_height(34.)
        .frame(Frame::NONE.fill(Color32::from_rgb(25, 25, 29)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let controls = bar.right() - 132.;
            let drag = Rect::from_min_max(bar.min, pos2(controls, bar.bottom()));
            let response = ui.interact(drag, ui.id().with("move"), Sense::click_and_drag());
            if response.double_clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            } else if response.drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            let p = ui.painter();
            let cy = bar.center().y;
            let x = bar.left() + 15.;
            for shift in [0., 7.] {
                p.add(Shape::convex_polygon(
                    vec![
                        pos2(x + shift, cy + 6.),
                        pos2(x + shift + 5., cy - 6.),
                        pos2(x + shift + 10., cy - 6.),
                        pos2(x + shift + 5., cy + 6.),
                    ],
                    accent,
                    Stroke::NONE,
                ));
            }
            p.text(
                pos2(x + 27., cy),
                Align2::LEFT_CENTER,
                "RIFT ATELIER",
                bold(12.),
                TEXT,
            );
            p.text(
                pos2(x + 119., cy),
                Align2::LEFT_CENTER,
                env!("CARGO_PKG_VERSION"),
                mono(10.),
                MUTED,
            );
            p.hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1_f32, LINE));
            for (i, label) in [
                "Свернуть",
                if maximized {
                    "Восстановить"
                } else {
                    "Развернуть"
                },
                "Закрыть",
            ]
            .iter()
            .enumerate()
            {
                let r = Rect::from_min_size(
                    pos2(controls + i as f32 * 44., bar.top()),
                    vec2(44., bar.height()),
                );
                let hit = ui
                    .interact(r, ui.id().with(i), Sense::click())
                    .on_hover_text(*label);
                if hit.hovered() {
                    p.rect_filled(
                        r,
                        0.,
                        if i == 2 {
                            Color32::from_rgb(155, 49, 50)
                        } else {
                            LINE
                        },
                    );
                }
                let c = r.center();
                let pen = Stroke::new(1.25_f32, TEXT);
                match i {
                    0 => {
                        p.line_segment([c - vec2(5., 0.), c + vec2(5., 0.)], pen);
                    }
                    1 => {
                        let icon = Rect::from_center_size(c, vec2(10., 9.));
                        if maximized {
                            p.line_segment(
                                [
                                    icon.left_top() + vec2(2., -2.),
                                    icon.right_top() + vec2(2., -2.),
                                ],
                                pen,
                            );
                            p.line_segment(
                                [
                                    icon.right_top() + vec2(2., -2.),
                                    icon.right_bottom() + vec2(2., -2.),
                                ],
                                pen,
                            );
                        }
                        p.rect_stroke(icon, 0., pen, StrokeKind::Inside);
                    }
                    _ => {
                        p.line_segment([c - vec2(4.5, 4.5), c + vec2(4.5, 4.5)], pen);
                        p.line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], pen);
                    }
                }
                if hit.clicked() {
                    ctx.send_viewport_cmd(match i {
                        0 => ViewportCommand::Minimized(true),
                        1 => ViewportCommand::Maximized(!maximized),
                        _ => ViewportCommand::Close,
                    });
                }
            }
        });
}
