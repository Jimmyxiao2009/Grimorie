//! Handwritten ink.
//!
//! Ink is an *annotation kind*, not a parallel feature. A handwritten margin
//! note is an [`Annotation`] whose `kind` is [`AnnotationKind::Ink`] and whose
//! vector content lives here. Reusing the annotation system gives ink the
//! anchoring, staleness, and cascade behaviour the Margin already has — the
//! only thing ink adds is stroke data.
//!
//! # Coordinate space
//!
//! Strokes are stored in a **logical, surface-relative** space so handwriting
//! does not drift when the window is resized, the Margin opens or closes, or
//! the device rotates:
//!
//! * `x` is **normalised** to `[0, 1]` against the ink surface's width. Because
//!   the Margin width changes with layout and theme, an absolute pixel x would
//!   land in a different place after a resize; a fraction of the surface stays
//!   put.
//! * `y` is an **absolute pixel offset** from the top of the ink surface. The
//!   surface's height grows downward as the writer writes, so the surface never
//!   has a fixed height to normalise against — and vertical position is what
//!   keeps a note level with the prose beside it.
//!
//! This is the same trade the rest of the Margin makes: anchored notes track
//! vertical position, and the only horizontal movement is the surface itself
//! resizing as a unit.

use serde::{Deserialize, Serialize};

use super::ids::StrokeId;
use super::manuscript::Timestamp;

/// The tool a stroke was drawn with. Two are enough for marginalia; a wider
/// palette would turn the Margin into a drawing program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InkTool {
    /// Ordinary handwriting.
    Pen,
    /// A translucent, wider mark meant to sit behind the pen.
    Highlighter,
}

impl InkTool {
    pub fn as_str(&self) -> &'static str {
        match self {
            InkTool::Pen => "pen",
            InkTool::Highlighter => "highlighter",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "pen" => InkTool::Pen,
            "highlighter" => InkTool::Highlighter,
            _ => return None,
        })
    }
}

/// One sampled point of a stroke.
///
/// See the module docs for what `x` and `y` mean. `pressure` and `timestamp`
/// are kept when the hardware offers them; both are `Option` so a mouse or a
/// WebView that does not expose pressure still round-trips cleanly. Tilt is
/// deliberately absent from the MVP — it is rarely available through a WebView
/// and adds weight to every point for little marginal benefit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkPoint {
    /// Normalised horizontal position, `[0, 1]`, across the ink surface.
    pub x: f32,
    /// Absolute vertical position, in surface pixels, from the top.
    pub y: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure: Option<f32>,
    /// Monotonic milliseconds since some epoch, as reported by the pointer.
    /// Optional because older data and synthetic pointers may not carry one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

impl InkPoint {
    /// A point with no pressure or timing, for tests and synthetic input.
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            pressure: None,
            timestamp: None,
        }
    }
}

/// A continuous vector stroke.
///
/// Rendered as one SVG path, never one element per point. The `color` is a
/// semantic id resolved per theme (e.g. `"ink-primary"`) so handwriting stays
/// visible in every theme without the writer choosing a colour per theme; a
/// raw CSS colour is also accepted for custom picks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkStroke {
    pub id: StrokeId,
    pub tool: InkTool,
    /// A semantic colour id, or a raw CSS colour for custom picks.
    pub color: String,
    /// Base stroke width in surface pixels. Pressure modulates around it.
    pub width: f32,
    pub points: Vec<InkPoint>,
    pub created_at: Timestamp,
}

impl InkStroke {
    /// Mints a stroke from raw points. The caller supplies the points as drawn;
    /// smoothing is a render-time concern, not a storage one, so the stored
    /// points are exactly what the pen produced.
    pub fn new(tool: InkTool, color: &str, width: f32, points: Vec<InkPoint>) -> Self {
        Self {
            id: StrokeId::new(),
            tool,
            color: color.to_string(),
            width,
            points,
            created_at: super::manuscript::now(),
        }
    }

    /// The minimum number of points a stroke needs to be visible.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// A single point is drawn as a dot so a tap still leaves a mark.
    pub fn is_dot(&self) -> bool {
        self.points.len() == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_round_trip_through_text() {
        for tool in [InkTool::Pen, InkTool::Highlighter] {
            assert_eq!(InkTool::parse(tool.as_str()), Some(tool));
        }
        assert_eq!(InkTool::parse("paintbrush"), None);
    }

    #[test]
    fn a_stroke_mints_a_unique_id_and_keeps_its_points() {
        let points = vec![InkPoint::new(0.1, 5.0), InkPoint::new(0.2, 6.0)];
        let stroke = InkStroke::new(InkTool::Pen, "ink-primary", 2.0, points.clone());
        assert!(!stroke.is_empty());
        assert!(!stroke.is_dot());
        assert_eq!(stroke.points, points);
        assert_ne!(stroke.id, StrokeId::new());
    }

    #[test]
    fn a_single_point_stroke_is_a_dot() {
        let stroke = InkStroke::new(
            InkTool::Pen,
            "ink-primary",
            2.0,
            vec![InkPoint::new(0.5, 5.0)],
        );
        assert!(stroke.is_dot());
        assert!(!stroke.is_empty());
    }

    #[test]
    fn a_stroke_with_no_points_is_empty() {
        let stroke = InkStroke::new(InkTool::Pen, "ink-primary", 2.0, vec![]);
        assert!(stroke.is_empty());
    }

    #[test]
    fn points_with_pressure_and_timestamp_round_trip() {
        let point = InkPoint {
            x: 0.25,
            y: 12.0,
            pressure: Some(0.8),
            timestamp: Some(1_000),
        };
        let json = serde_json::to_string(&point).unwrap();
        // Optional fields are present when set.
        assert!(json.contains("pressure"));
        assert!(json.contains("timestamp"));
        let back: InkPoint = serde_json::from_str(&json).unwrap();
        assert_eq!(point, back);
    }

    #[test]
    fn points_without_extras_omit_the_optional_fields() {
        let point = InkPoint::new(0.25, 12.0);
        let json = serde_json::to_string(&point).unwrap();
        assert!(!json.contains("pressure"));
        assert!(!json.contains("timestamp"));
    }
}
