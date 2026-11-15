use super::{Position, position::Side, state::BoardState};
use crate::theme;
use eframe::egui::{self, Align2, FontFamily, FontId, Rect, Sense, Stroke, StrokeKind, Vec2};

const COORDINATE_MARGIN: f32 = 22.0;

pub fn install_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "chess".into(),
        egui::FontData::from_static(include_bytes!(
            "../../../../assets/fonts/mfalmion-chess.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .insert(FontFamily::Name("Chess".into()), vec!["chess".into()]);
    ctx.set_fonts(fonts);
}

/// Paint a square board, with input hit targets using the very same mapped rectangles.
/// Position is read-only: selection and flipping can never move a piece.
pub fn show(ui: &mut egui::Ui, position: &Position, state: &mut BoardState, desired_side: f32) {
    let side = desired_side.min((ui.available_width() - 2.0 * COORDINATE_MARGIN).max(1.0));
    let (outer, _) =
        ui.allocate_exact_size(Vec2::splat(side + 2.0 * COORDINATE_MARGIN), Sense::hover());
    let board = outer.shrink(COORDINATE_MARGIN);
    let cell = side / 8.0;
    ui.painter().rect_filled(outer, 8, theme::SURFACE);
    for row in 0..8 {
        for column in 0..8 {
            let square = state
                .orientation
                .square_at(column, row)
                .expect("8×8 display bounds");
            let rect = Rect::from_min_size(
                board.min + egui::vec2(f32::from(column) * cell, f32::from(row) * cell),
                Vec2::splat(cell),
            );
            let response = ui
                .interact(
                    rect,
                    ui.id().with(("square", square.index())),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                state.select(square);
                ui.ctx().request_repaint();
            }
            let painter = ui.painter();
            painter.rect_filled(
                rect,
                0,
                if square.is_dark() {
                    theme::BOARD_DARK
                } else {
                    theme::BOARD_LIGHT
                },
            );
            if state.selected == Some(square) {
                painter.rect_filled(rect, 0, theme::CYAN.linear_multiply(0.24));
            }
            if let Some(piece) = position.piece_at(square) {
                let (fill, outline) = match piece.side {
                    Side::White => (theme::PIECE_LIGHT, theme::PIECE_DARK),
                    Side::Black => (theme::PIECE_DARK, theme::PIECE_LIGHT),
                };
                let font = FontId::new(cell * 0.83, FontFamily::Name("Chess".into()));
                let galley = painter.layout_no_wrap(piece.glyph().into(), font, fill);
                let origin = rect.center() - galley.size() * 0.5;
                let thickness = (cell * 0.018).clamp(0.7, 1.5);
                for offset in [
                    egui::vec2(-1.0, 0.0),
                    egui::vec2(1.0, 0.0),
                    egui::vec2(0.0, -1.0),
                    egui::vec2(0.0, 1.0),
                ] {
                    painter.galley_with_override_text_color(
                        origin + offset * thickness,
                        galley.clone(),
                        outline,
                    );
                }
                painter.galley(origin, galley, fill);
            }
            if state.selected == Some(square) {
                painter.rect_stroke(
                    rect.shrink(2.0),
                    0,
                    Stroke::new(3.0, theme::CYAN),
                    StrokeKind::Inside,
                );
            } else if response.hovered() || response.has_focus() {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    0,
                    Stroke::new(1.5, theme::PURPLE),
                    StrokeKind::Inside,
                );
            }
            response.on_hover_text(square.to_string());
        }
    }
    let painter = ui.painter();
    painter.rect_stroke(
        board,
        0,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Outside,
    );
    let font = FontId::proportional(12.0);
    for index in 0..8 {
        let bottom = state.orientation.square_at(index, 7).expect("valid file");
        let left = state.orientation.square_at(0, index).expect("valid rank");
        painter.text(
            egui::pos2(
                board.left() + (f32::from(index) + 0.5) * cell,
                board.bottom() + 12.0,
            ),
            Align2::CENTER_CENTER,
            char::from(b'a' + bottom.file()),
            font.clone(),
            theme::MUTED,
        );
        painter.text(
            egui::pos2(
                board.left() - 12.0,
                board.top() + (f32::from(index) + 0.5) * cell,
            ),
            Align2::CENTER_CENTER,
            (left.rank() + 1).to_string(),
            font.clone(),
            theme::MUTED,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{square::Square, state::Orientation};

    /// CPU-only egui passes: no native window, display connection, or GPU initialization.
    fn frame(
        ctx: &egui::Context,
        position: &Position,
        state: &mut BoardState,
        side: f32,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::splat(side + 44.0),
            )),
            events,
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| show(ui, position, state, side));
        assert!(!output.shapes.is_empty());
        let squares: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Rect(rect) = &shape.shape
                    && (rect.fill == theme::BOARD_LIGHT || rect.fill == theme::BOARD_DARK)
                {
                    Some(rect.rect)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(squares.len(), 64);
        for rect in squares {
            assert!((rect.width() - side / 8.0).abs() < 0.01);
            assert!((rect.height() - side / 8.0).abs() < 0.01);
        }
        output.drop_without_applying_deltas();
    }

    fn click(
        ctx: &egui::Context,
        position: &Position,
        state: &mut BoardState,
        side: f32,
        column: u8,
        row: u8,
    ) {
        let pos = egui::pos2(
            COORDINATE_MARGIN + (f32::from(column) + 0.5) * side / 8.0,
            COORDINATE_MARGIN + (f32::from(row) + 0.5) * side / 8.0,
        );
        for pressed in [true, false] {
            frame(
                ctx,
                position,
                state,
                side,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn rendered_hit_targets_match_both_orientations_at_two_sizes() {
        for side in [280.0, 640.0] {
            let ctx = egui::Context::default();
            install_font(&ctx);
            let position = Position::default();
            let mut state = BoardState::default();
            frame(&ctx, &position, &mut state, side, vec![]);
            click(&ctx, &position, &mut state, side, 0, 7);
            assert_eq!(state.selected, Square::new(0, 0)); // a1
            click(&ctx, &position, &mut state, side, 0, 7);
            assert_eq!(state.selected, None);
            click(&ctx, &position, &mut state, side, 3, 7);
            assert_eq!(state.selected, Square::new(3, 0)); // d1
            state.flip();
            assert_eq!(state.orientation, Orientation::BlackBottom);
            frame(&ctx, &position, &mut state, side, vec![]);
            click(&ctx, &position, &mut state, side, 0, 0);
            assert_eq!(state.selected, Square::new(7, 0)); // h1 after flip
            // Selection and orientation never modify the position snapshot.
            assert_eq!(
                position
                    .piece_at(Square::new(3, 0).unwrap())
                    .unwrap()
                    .glyph(),
                "♛"
            );
        }
    }
}
