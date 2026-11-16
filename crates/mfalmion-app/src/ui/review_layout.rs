//! Pure presentation geometry, colors and persisted critical-move navigation.
use crate::analysis::{Summary, classify::Category};
use eframe::egui::Color32;
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub left: f32,
    pub right: f32,
    pub board: f32,
    pub height: f32,
    pub compact: bool,
}
impl Layout {
    pub fn calculate(width: f32, height: f32) -> Self {
        let width = width.max(1.0);
        let height = height.max(1.0);
        let gap = 12.0_f32.min(width * 0.02);
        let usable = (width - gap).max(1.0);
        let compact = width < 900.0 || height < 590.0;
        let left = (usable * 0.60)
            .min((height - 56.0).max(1.0))
            .min((usable - 260.0).max(usable * 0.35));
        Self {
            left,
            right: (usable - left).max(1.0),
            board: (left - 44.0).min((height - 100.0).max(1.0)).max(1.0),
            height,
            compact,
        }
    }
}
pub fn color(category: Category) -> Color32 {
    match category {
        Category::Brilliant => Color32::from_rgb(68, 221, 214),
        Category::Great => Color32::from_rgb(116, 175, 255),
        Category::Best => Color32::from_rgb(111, 207, 128),
        Category::Excellent => Color32::from_rgb(183, 226, 153),
        Category::Good => Color32::from_rgb(214, 218, 226),
        Category::Inaccuracy => Color32::from_rgb(244, 216, 92),
        Category::Mistake => Color32::from_rgb(255, 169, 91),
        Category::Blunder => Color32::from_rgb(255, 116, 128),
        Category::Miss => Color32::from_rgb(219, 151, 255),
        Category::Unclassified => Color32::from_rgb(170, 180, 195),
    }
}
pub fn critical_target(
    summary: Option<&Summary>,
    cursor: usize,
    forward: bool,
    mistakes_only: bool,
) -> Option<usize> {
    let matches = |c| {
        if mistakes_only {
            matches!(c, Category::Mistake | Category::Blunder)
        } else {
            matches!(
                c,
                Category::Brilliant
                    | Category::Great
                    | Category::Miss
                    | Category::Mistake
                    | Category::Blunder
            )
        }
    };
    let moves = &summary?.moves;
    if forward {
        moves
            .iter()
            .find(|m| m.ply > cursor && matches(m.classification))
            .map(|m| m.ply)
    } else {
        moves
            .iter()
            .rev()
            .find(|m| m.ply < cursor && matches(m.classification))
            .map(|m| m.ply)
    }
}
pub fn accuracy(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{v:.1}%"))
}
pub fn saved_label(summary: &Summary) -> String {
    format!(
        "Saved batch · {} · depth {} · MultiPV {}",
        summary.config.identity, summary.config.engine.depth, summary.config.engine.multipv
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_desktop_sizes_fit_square_board_and_wider_sidebar() {
        for (width, height) in [
            (1920.0, 1080.0),
            (1440.0, 900.0),
            (1366.0, 768.0),
            (1280.0, 720.0),
        ] {
            // Existing app sidebar and panel margins, plus conservative toolbar/status allowance.
            let width = width - 232.0 - 56.0;
            let height = height - 240.0;
            let l = Layout::calculate(width, height);
            assert!(l.board > 300.0, "board shrank too far at {width}");
            assert!(l.right >= 300.0);
            assert!(l.left + l.right <= width);
            assert!(l.board + 44.0 <= l.left + 0.01);
            assert!(l.board + 100.0 <= height + 0.01);
            assert!(l.height <= height);
        }
    }
    #[test]
    fn narrower_windows_keep_regions_in_bounds() {
        for width in [440.0, 540.0, 660.0, 800.0] {
            let l = Layout::calculate(width, 420.0);
            assert!(l.left + l.right <= width);
            assert!(l.board > 0.0);
            assert!(l.right >= 200.0);
            assert!(l.compact);
        }
    }
    #[test]
    fn accuracy_labels_do_not_invent_missing_values() {
        assert_eq!(accuracy(Some(94.34)), "94.3%");
        assert_eq!(accuracy(None), "—");
    }
    #[test]
    fn all_categories_have_distinct_accessible_text_labels() {
        use crate::analysis::classify::Category as C;
        for c in [
            C::Brilliant,
            C::Great,
            C::Best,
            C::Excellent,
            C::Good,
            C::Inaccuracy,
            C::Mistake,
            C::Blunder,
            C::Miss,
            C::Unclassified,
        ] {
            assert!(!c.label().is_empty());
            assert_ne!(color(c), Color32::TRANSPARENT);
        }
    }
}
