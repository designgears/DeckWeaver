//! Text rasterisation for the renderers.
//!
//! Inter Bold is embedded rather than loaded from the system: the previous implementation
//! probed a hard-coded list of `/usr/share/fonts` paths and silently drew nothing when none
//! matched, so the strip could come out blank depending on the distro.
//!
//! Only one weight is bundled, and it is a heavy one. Everything on the strip is 12–16px on a
//! physically small LCD and has to stay readable over whatever background the user set; lighter
//! weights render thin and wash out. Hierarchy comes from size and colour instead.

use ab_glyph::{point, Font, FontArc, GlyphId, PxScale, ScaleFont};
use image::imageops::FilterType;
use image::{Rgba as ImageRgba, RgbaImage};
use imageproc::drawing::draw_text_mut;
use std::sync::OnceLock;
use tiny_skia::Pixmap;

use super::common::{blit_alpha_tinted, blit_outline, Rect, Rgba};
use super::theme;

/// Glyphs are rasterised at this multiple and downsampled, which is what keeps 12px labels
/// readable on the LCD.
const SUPERSAMPLE: u32 = 2;

/// Downsampling the supersampled raster leaves edge pixels lighter than a hinted renderer would
/// produce, which reads as thin at these sizes. Boosting coverage restores the apparent stroke
/// weight without touching glyph geometry.
const COVERAGE_GAMMA: f32 = 1.4;

const INTER_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.ttf");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    pub size: f32,
    pub color: Rgba,
    pub align: TextAlign,
    /// Dark surround behind the glyphs. Only turn this off for text sitting on an opaque fill.
    pub outline: bool,
}

impl TextStyle {
    pub const fn new(size: f32, color: Rgba, align: TextAlign) -> Self {
        Self {
            size,
            color,
            align,
            outline: true,
        }
    }

    pub fn without_outline(mut self) -> Self {
        self.outline = false;
        self
    }
}

fn font() -> &'static FontArc {
    static FONT: OnceLock<FontArc> = OnceLock::new();
    FONT.get_or_init(|| FontArc::try_from_slice(INTER_BOLD).expect("bundled Inter Bold"))
}

/// Advance width of `text`, in pixels, including kerning.
pub fn measure_text(text: &str, size: f32) -> f32 {
    let scaled = font().as_scaled(PxScale::from(size));
    let mut width = 0.0;
    let mut previous: Option<GlyphId> = None;

    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(prev) = previous {
            width += scaled.kern(prev, id);
        }
        width += scaled.h_advance(id);
        previous = Some(id);
    }

    width
}

/// Shorten `text` with a trailing ellipsis until it fits `max_width`.
///
/// Widths are summed per character, so kerning between the kept characters is ignored — the
/// result is very slightly conservative, which is the safe direction for a fixed-width slot.
pub fn truncate_to_width(text: &str, max_width: f32, size: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if measure_text(text, size) <= max_width {
        return text.to_string();
    }

    let ellipsis_width = measure_text("…", size);
    let mut kept = String::new();
    let mut width = 0.0;

    for c in text.chars() {
        let char_width = measure_text(c.encode_utf8(&mut [0u8; 4]), size);
        if width + char_width + ellipsis_width > max_width {
            break;
        }
        kept.push(c);
        width += char_width;
    }

    while kept.ends_with(' ') {
        kept.pop();
    }
    kept.push('…');
    kept
}

/// Draw `text` inside `rect`, aligned per `style` and vertically centred.
pub fn draw_text(pixmap: &mut Pixmap, text: &str, rect: Rect, style: &TextStyle) {
    if text.is_empty() {
        return;
    }
    let Some(mask) = rasterize(text, rect, style) else {
        return;
    };

    // The outline is padded into the mask, so shift back by the same amount.
    let x = rect.x.round() as i32 - OUTLINE_PAD as i32;
    let y = rect.y.round() as i32 - OUTLINE_PAD as i32;

    if style.outline {
        blit_outline(pixmap, &mask, x, y, &theme::CONTENT_OUTLINE);
    }
    blit_alpha_tinted(pixmap, &mask, x, y, style.color);
}

/// Slack around the text slot so a glyph touching the slot edge still gets its full outline.
const OUTLINE_PAD: u32 = 4;

/// Rasterise `text` into a coverage mask. Only the alpha channel is meaningful; callers tint it
/// via [`blit_alpha_tinted`], which is what lets one raster pass serve the outline, the drop
/// shadow and the glyphs themselves.
fn rasterize(text: &str, rect: Rect, style: &TextStyle) -> Option<RgbaImage> {
    let font = font();
    let scale = PxScale::from(style.size * SUPERSAMPLE as f32);
    let pad = OUTLINE_PAD * SUPERSAMPLE;
    let slot_w = (rect.w.ceil().max(1.0) as u32) * SUPERSAMPLE;
    let slot_h = (rect.h.ceil().max(1.0) as u32) * SUPERSAMPLE;
    let width = slot_w + pad * 2;
    let height = slot_h + pad * 2;

    let (min_x, min_y, max_x, max_y) = text_pixel_bounds(font, scale, text)?;
    let text_w = (max_x - min_x).ceil().max(1.0);
    let text_h = (max_y - min_y).ceil().max(1.0);

    let text_x = (pad as f32
        + match style.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (slot_w as f32 - text_w) * 0.5,
            TextAlign::Right => slot_w as f32 - text_w,
        }
        - min_x)
        .round() as i32;
    let text_y = (pad as f32 + (slot_h as f32 - text_h) * 0.5 - min_y).round() as i32;

    let mut mask = RgbaImage::from_pixel(width, height, ImageRgba([0, 0, 0, 0]));
    draw_text_mut(
        &mut mask,
        ImageRgba([255, 255, 255, 255]),
        text_x,
        text_y,
        scale,
        font,
        text,
    );

    let mut scaled = image::imageops::resize(
        &mask,
        width / SUPERSAMPLE,
        height / SUPERSAMPLE,
        FilterType::Lanczos3,
    );
    for pixel in scaled.pixels_mut() {
        let coverage = pixel[3] as f32 / 255.0;
        if coverage > 0.0 && coverage < 1.0 {
            pixel[3] = ((1.0 - (1.0 - coverage).powf(COVERAGE_GAMMA)) * 255.0).round() as u8;
        }
    }

    Some(scaled)
}

fn text_pixel_bounds(font: &FontArc, scale: PxScale, text: &str) -> Option<(f32, f32, f32, f32)> {
    let scaled = font.as_scaled(scale);
    let mut pen_x = 0.0f32;
    let mut previous: Option<GlyphId> = None;
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(prev) = previous {
            pen_x += scaled.kern(prev, id);
        }
        previous = Some(id);

        let glyph = id.with_scale_and_position(scale, point(pen_x, scaled.ascent()));
        pen_x += scaled.h_advance(id);

        if let Some(outlined) = scaled.outline_glyph(glyph) {
            let bb = outlined.px_bounds();
            min_x = min_x.min(bb.min.x);
            min_y = min_y.min(bb.min.y);
            max_x = max_x.max(bb.max.x);
            max_y = max_y.max(bb.max.y);
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_loads() {
        assert!(measure_text("Chat", 15.0) > 0.0);
    }

    #[test]
    fn short_text_is_untouched() {
        let width = measure_text("Chat", 15.0);
        assert_eq!(truncate_to_width("Chat", width + 1.0, 15.0), "Chat");
    }

    #[test]
    fn long_text_is_ellipsised_and_fits() {
        let name = "Headphones (USB Audio)";
        let truncated = truncate_to_width(name, 96.0, 15.0);

        assert!(truncated.ends_with('…'));
        assert!(truncated.chars().count() < name.chars().count());
        assert!(measure_text(&truncated, 15.0) <= 96.0);
    }

    #[test]
    fn impossibly_narrow_slot_yields_just_an_ellipsis() {
        assert_eq!(truncate_to_width("Chat", 1.0, 15.0), "…");
    }
}
