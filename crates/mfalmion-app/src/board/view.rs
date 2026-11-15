use super::{
    AnimationState, Position, Square,
    animation::square_center,
    artwork::PieceTextures,
    position::Side,
    state::{BoardState, Orientation},
};
use crate::theme;
use eframe::egui::{self, Align2, FontId, Rect, Sense, Stroke, StrokeKind, Vec2};

const COORDINATE_MARGIN: f32 = 22.0;

#[derive(Default)]
pub struct Highlights<'a> {
    pub legal_targets: &'a [Square],
    pub capture_targets: &'a [Square],
    pub checked_king: Option<Square>,
    pub animations: Option<&'a AnimationState>,
    pub draggable_side: Option<Side>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoardInput {
    Click(Square),
    DragStart(Square),
    Drop {
        from: Square,
        to: Option<Square>,
        at: [f32; 2],
    },
}

struct Geometry {
    board: Rect,
    orientation: Orientation,
}
impl Geometry {
    fn cell(&self) -> f32 {
        self.board.width() / 8.0
    }
    fn point(&self, canonical: [f32; 2]) -> egui::Pos2 {
        let display = self.orientation.project(canonical);
        self.board.min + egui::vec2(display[0], display[1]) * self.cell()
    }
    fn rect(&self, square: Square) -> Rect {
        Rect::from_center_size(self.point(square_center(square)), Vec2::splat(self.cell()))
    }
    fn canonical(&self, point: egui::Pos2) -> [f32; 2] {
        let display = (point - self.board.min) / self.cell();
        self.orientation.unproject([display.x, display.y])
    }
    fn hit(&self, point: egui::Pos2) -> Option<Square> {
        if !self.board.contains(point) {
            return None;
        }
        let display = (point - self.board.min) / self.cell();
        self.orientation
            .square_at(display.x.floor() as u8, display.y.floor() as u8)
    }
}

fn squares() -> impl Iterator<Item = Square> {
    (0..64).map(|index| Square::new(index % 8, index / 8).expect("board bounds"))
}

/// Painting never changes the position. Input and animation coordinates share one geometry.
pub fn show(
    ui: &mut egui::Ui,
    position: &Position,
    state: &BoardState,
    desired_side: f32,
    textures: &mut PieceTextures,
    highlights: Highlights<'_>,
) -> Option<BoardInput> {
    let now = ui.input(|input| input.time);
    let mut action = None;
    let mut dragging = None;
    let mut hovered = None;
    let mut focused = None;
    let side = desired_side.min((ui.available_width() - 2.0 * COORDINATE_MARGIN).max(1.0));
    let (outer, _) =
        ui.allocate_exact_size(Vec2::splat(side + 2.0 * COORDINATE_MARGIN), Sense::hover());
    let geometry = Geometry {
        board: outer.shrink(COORDINATE_MARGIN),
        orientation: state.orientation,
    };
    let cell = geometry.cell();
    ui.painter().rect_filled(outer, 8, theme::SURFACE);

    // 1. Squares and input collection. No pieces are painted in this pass.
    for square in squares() {
        let rect = geometry.rect(square);
        ui.painter().rect_filled(
            rect,
            0,
            if square.is_dark() {
                theme::BOARD_DARK
            } else {
                theme::BOARD_LIGHT
            },
        );
        let piece = position.piece_at(square);
        let draggable = piece.is_some_and(|p| Some(p.side) == highlights.draggable_side);
        let sense = if draggable {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let response = ui
            .interact(rect, ui.id().with(("square", square.index())), sense)
            .on_hover_cursor(if draggable {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::PointingHand
            });
        if response.clicked() {
            action = Some(BoardInput::Click(square));
        }
        if response.drag_started() {
            action = Some(BoardInput::DragStart(square));
        }
        if response.drag_stopped()
            && let Some(pos) = response.interact_pointer_pos()
        {
            action = Some(BoardInput::Drop {
                from: square,
                to: geometry.hit(pos),
                at: geometry.canonical(pos),
            });
        }
        if (response.dragged() || response.drag_stopped())
            && let (Some(piece), Some(pos)) = (piece, response.interact_pointer_pos())
        {
            dragging = Some((square, piece, pos));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if response.hovered() {
            hovered = Some(square);
        }
        if response.has_focus() {
            focused = Some(square);
        }
        response.on_hover_text(square.to_string());
    }

    // 2. Selection, legal targets, capture rings and check background.
    let painter = ui.painter();
    for square in squares() {
        let rect = geometry.rect(square);
        if state.selected == Some(square) {
            painter.rect_filled(rect, 0, theme::CYAN.linear_multiply(0.24));
        }
        if highlights.checked_king == Some(square) {
            painter.rect_filled(rect, 0, theme::DANGER.linear_multiply(0.35));
        }
        if highlights.legal_targets.contains(&square) {
            if highlights.capture_targets.contains(&square) {
                painter.circle_stroke(
                    rect.center(),
                    cell * 0.42,
                    Stroke::new(5.0, theme::BACKGROUND),
                );
                painter.circle_stroke(rect.center(), cell * 0.42, Stroke::new(2.5, theme::MINT));
            } else {
                painter.circle_filled(rect.center(), cell * 0.08, theme::MINT);
                painter.circle_stroke(
                    rect.center(),
                    cell * 0.08,
                    Stroke::new(1.5, theme::BACKGROUND),
                );
            }
        }
    }

    // 3. Stationary pieces, excluding the drag source and each animated destination.
    for square in squares() {
        if dragging.is_some_and(|(from, _, _)| from == square)
            || highlights.animations.is_some_and(|a| a.hides(square))
        {
            continue;
        }
        if let Some(piece) = position.piece_at(square) {
            textures.paint(painter, geometry.rect(square).center(), piece, cell);
        }
    }
    // 4. Animated movers and the cursor-centered dragged piece always sit above stationary pieces.
    if let Some(animations) = highlights.animations {
        for track in animations.tracks() {
            textures.paint(painter, geometry.point(track.point(now)), track.piece, cell);
        }
        if animations.is_active() {
            ui.ctx().request_repaint();
        }
    }
    if let Some((_, piece, pos)) = dragging {
        textures.paint(painter, pos, piece, cell);
    }

    // 5. Relevant overlays and coordinates, after every piece has been painted.
    for square in squares() {
        let rect = geometry.rect(square);
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
        } else if hovered == Some(square) || focused == Some(square) {
            painter.rect_stroke(
                rect.shrink(1.0),
                0,
                Stroke::new(1.5, theme::PURPLE),
                StrokeKind::Inside,
            );
        }
        if highlights.checked_king == Some(square) {
            painter.circle_filled(
                rect.right_top() + egui::vec2(-cell * 0.12, cell * 0.12),
                cell * 0.06,
                theme::DANGER,
            );
        }
    }
    let board = geometry.board;
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
    #[test]
    fn animation_geometry_reprojects_on_resize_and_flip() {
        use crate::board::animation::PieceAnimation;
        use crate::board::position::{Kind, Piece, Side};
        let from = Square::new(4, 1).unwrap();
        let to = Square::new(4, 3).unwrap();
        let animation = PieceAnimation::new(
            Piece {
                side: Side::White,
                kind: Kind::Pawn,
            },
            from,
            to,
            0.0,
            0.2,
        );
        let board = Rect::from_min_size(egui::pos2(100.0, 50.0), Vec2::splat(800.0));
        let geometry = Geometry {
            board,
            orientation: Orientation::WhiteBottom,
        };
        assert_eq!(
            geometry.point(animation.point(0.0)),
            egui::pos2(550.0, 700.0)
        );
        assert_eq!(
            geometry.point(animation.point(0.1)),
            egui::pos2(550.0, 525.0)
        );
        assert_eq!(
            geometry.point(animation.point(0.2)),
            egui::pos2(550.0, 500.0)
        );
        let smaller = Geometry {
            board: Rect::from_min_size(board.min, Vec2::splat(400.0)),
            orientation: Orientation::WhiteBottom,
        };
        assert_eq!(
            smaller.point(animation.point(0.1)),
            egui::pos2(325.0, 287.5)
        );
        let flipped = Geometry {
            board,
            orientation: Orientation::BlackBottom,
        };
        assert_eq!(
            flipped.point(animation.point(0.0)),
            egui::pos2(450.0, 200.0)
        );
        assert_eq!(flipped.hit(flipped.point(animation.point(0.2))), Some(to));
        for geometry in [geometry, smaller, flipped] {
            let outside = [-1.0, 9.0];
            assert_eq!(geometry.canonical(geometry.point(outside)), outside);
            assert!(geometry.hit(geometry.point(outside)).is_none());
        }
    }
}
