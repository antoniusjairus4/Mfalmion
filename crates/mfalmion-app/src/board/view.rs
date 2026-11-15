use super::{Position, Square, artwork::PieceTextures, state::BoardState};
use crate::theme;
use eframe::egui::{self, Align2, FontId, Rect, Sense, Stroke, StrokeKind, Vec2};

const COORDINATE_MARGIN: f32 = 22.0;

#[derive(Default)]
pub struct Highlights<'a> {
    pub legal_targets: &'a [Square],
    pub capture_targets: &'a [Square],
    pub checked_king: Option<Square>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardInput {
    Click(Square),
    DragStart(Square),
    Drop { from: Square, to: Square },
}

/// Read-only painting and hit testing. The controller applies returned input through the rules library.
pub fn show(
    ui: &mut egui::Ui,
    position: &Position,
    state: &BoardState,
    desired_side: f32,
    textures: &mut PieceTextures,
    highlights: Highlights<'_>,
) -> Option<BoardInput> {
    let mut action = None;
    let mut dragging = None;
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
                    Sense::click_and_drag(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                action = Some(BoardInput::Click(square));
            }
            if response.drag_started() {
                action = Some(BoardInput::DragStart(square));
            }
            if response.drag_stopped()
                && let Some(pos) = response.interact_pointer_pos()
                && board.contains(pos)
                && let Some(to) = state.orientation.square_at(
                    ((pos.x - board.left()) / cell).floor() as u8,
                    ((pos.y - board.top()) / cell).floor() as u8,
                )
            {
                action = Some(BoardInput::Drop { from: square, to });
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
            if highlights.checked_king == Some(square) {
                painter.rect_filled(rect, 0, theme::DANGER.linear_multiply(0.35));
                painter.circle_filled(
                    rect.right_top() + egui::vec2(-cell * 0.12, cell * 0.12),
                    cell * 0.06,
                    theme::DANGER,
                );
            }
            if highlights.legal_targets.contains(&square) {
                if highlights.capture_targets.contains(&square) {
                    painter.circle_stroke(
                        rect.center(),
                        cell * 0.42,
                        Stroke::new(5.0, theme::BACKGROUND),
                    );
                    painter.circle_stroke(
                        rect.center(),
                        cell * 0.42,
                        Stroke::new(2.5, theme::MINT),
                    );
                } else {
                    painter.circle_filled(rect.center(), cell * 0.08, theme::MINT);
                    painter.circle_stroke(
                        rect.center(),
                        cell * 0.08,
                        Stroke::new(1.5, theme::BACKGROUND),
                    );
                }
            }
            if let Some(piece) = position.piece_at(square) {
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    dragging = Some((piece, pos));
                } else {
                    textures.paint(painter, rect.center(), piece, cell);
                }
            }
            if state.selected == Some(square) {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    0,
                    Stroke::new(5.0, theme::BACKGROUND),
                    StrokeKind::Inside,
                );
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
    if let Some((piece, pos)) = dragging {
        textures.paint(ui.painter(), pos, piece, cell);
    }
    action
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
        let mut textures = PieceTextures::default();
        let output = ctx.run_ui(input, |ui| {
            if let Some(BoardInput::Click(square)) = show(
                ui,
                position,
                state,
                side,
                &mut textures,
                Highlights::default(),
            ) {
                state.select(square);
            }
        });
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
                position.piece_at(Square::new(3, 0).unwrap()).unwrap().kind,
                crate::board::position::Kind::Queen
            );
        }
    }
}
