//! Offline Cburnett SVGs, parsed once per piece and rasterized only on a size-bucket change.
use super::position::{Kind, Piece, Side};
use crate::theme;
use eframe::egui::{self, ColorImage, Rect, TextureHandle, TextureOptions, Vec2};
use resvg::{tiny_skia, usvg};

macro_rules! asset {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!(
                "../../../../assets/pieces/cburnett/",
                $name,
                ".svg"
            ))
            .as_slice(),
        )
    };
}
const ASSETS: [(&str, &[u8]); 12] = [
    asset!("wP"),
    asset!("wN"),
    asset!("wB"),
    asset!("wR"),
    asset!("wQ"),
    asset!("wK"),
    asset!("bP"),
    asset!("bN"),
    asset!("bB"),
    asset!("bR"),
    asset!("bQ"),
    asset!("bK"),
];

fn asset_index(piece: Piece) -> usize {
    let side = match piece.side {
        Side::White => 0,
        Side::Black => 6,
    };
    side + match piece.kind {
        Kind::Pawn => 0,
        Kind::Knight => 1,
        Kind::Bishop => 2,
        Kind::Rook => 3,
        Kind::Queen => 4,
        Kind::King => 5,
    }
}

fn raster_size(logical_side: f32, pixels_per_point: f32) -> u32 {
    // Round up to limit rerasterization during resizing. One live size per piece bounds memory.
    ((logical_side * pixels_per_point).ceil().clamp(16.0, 512.0) as u32).next_multiple_of(16)
}

fn rasterize(tree: &usvg::Tree, pixels: u32) -> Result<ColorImage, &'static str> {
    let mut pixmap = tiny_skia::Pixmap::new(pixels, pixels).ok_or("SVG pixel allocation failed")?;
    let scale = tiny_skia::Transform::from_scale(
        pixels as f32 / tree.size().width(),
        pixels as f32 / tree.size().height(),
    );
    resvg::render(tree, scale, &mut pixmap.as_mut());
    // tiny-skia uses premultiplied RGBA; preserve alpha to avoid fringes on either board color.
    Ok(ColorImage::from_rgba_premultiplied(
        [pixels as usize; 2],
        pixmap.data(),
    ))
}

struct CachedAsset {
    tree: Result<usvg::Tree, String>,
    texture: Option<(u32, Option<TextureHandle>)>,
}

#[derive(Default)]
pub struct PieceTextures {
    entries: [Option<CachedAsset>; 12],
}

impl PieceTextures {
    fn texture(&mut self, ctx: &egui::Context, piece: Piece, side: f32) -> Option<egui::TextureId> {
        let index = asset_index(piece);
        let (name, bytes) = ASSETS[index];
        let cached = self.entries[index].get_or_insert_with(|| {
            let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|error| {
                log::error!("event=piece_svg_parse_failed piece={name} error={error:?}");
                error.to_string()
            });
            CachedAsset {
                tree,
                texture: None,
            }
        });
        let tree = cached.tree.as_ref().ok()?;
        let pixels = raster_size(side, ctx.pixels_per_point());
        if cached
            .texture
            .as_ref()
            .is_none_or(|(size, _)| *size != pixels)
        {
            let texture = match rasterize(tree, pixels) {
                Ok(image) => Some(ctx.load_texture(
                    format!("cburnett-{name}"),
                    image,
                    TextureOptions::LINEAR,
                )),
                Err(error) => {
                    log::error!("event=piece_svg_render_failed piece={name} error={error:?}");
                    None
                }
            };
            // Replacing drops the previous handle, rather than retaining all past resize sizes.
            cached.texture = Some((pixels, texture));
        }
        cached.texture.as_ref()?.1.as_ref().map(TextureHandle::id)
    }

    pub fn paint(&mut self, painter: &egui::Painter, center: egui::Pos2, piece: Piece, cell: f32) {
        let side = cell * 0.96;
        let rect = Rect::from_center_size(center, Vec2::splat(side));
        if let Some(texture) = self.texture(painter.ctx(), piece, side) {
            painter.image(
                texture,
                rect,
                Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            // Visible error marker, never a silent Unicode-piece fallback.
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(cell * 0.5),
                theme::DANGER,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_twelve_assets_map_parse_and_render_with_transparency() {
        let mut names = std::collections::BTreeSet::new();
        for (side, prefix) in [(Side::White, 'w'), (Side::Black, 'b')] {
            for (kind, suffix) in [
                (Kind::Pawn, 'P'),
                (Kind::Knight, 'N'),
                (Kind::Bishop, 'B'),
                (Kind::Rook, 'R'),
                (Kind::Queen, 'Q'),
                (Kind::King, 'K'),
            ] {
                let (name, bytes) = ASSETS[asset_index(Piece { side, kind })];
                assert_eq!(name, format!("{prefix}{suffix}"));
                assert!(names.insert(name));
                let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).unwrap();
                for pixels in [32, 160] {
                    let image = rasterize(&tree, pixels).unwrap();
                    assert_eq!(image.size, [pixels as usize; 2]);
                    assert!(image.pixels.iter().any(|p| p.a() > 0));
                    assert!(image.pixels.iter().any(|p| p.a() == 0));
                }
            }
        }
        assert_eq!(names.len(), 12);
    }

    #[test]
    fn textures_are_reused_and_resize_cache_is_bounded() {
        // Texture management only: no OS window or GPU device.
        let ctx = egui::Context::default();
        let mut cache = PieceTextures::default();
        let piece = Piece {
            side: Side::White,
            kind: Kind::Queen,
        };
        let first = cache.texture(&ctx, piece, 70.0).unwrap();
        assert_eq!(cache.texture(&ctx, piece, 70.0), Some(first));
        assert_eq!(cache.texture(&ctx, piece, 72.0), Some(first)); // same 80px bucket
        assert_ne!(cache.texture(&ctx, piece, 100.0), Some(first));
        assert_eq!(cache.entries.iter().filter(|e| e.is_some()).count(), 1);
        for (side, _) in [(Side::White, 0), (Side::Black, 6)] {
            for kind in [
                Kind::Pawn,
                Kind::Knight,
                Kind::Bishop,
                Kind::Rook,
                Kind::Queen,
                Kind::King,
            ] {
                assert!(cache.texture(&ctx, Piece { side, kind }, 100.0).is_some());
            }
        }
        assert_eq!(cache.entries.iter().filter(|e| e.is_some()).count(), 12);
        assert_eq!(raster_size(80.0, 2.0), 160);
        assert_eq!(raster_size(10000.0, 2.0), 512);
    }
}
