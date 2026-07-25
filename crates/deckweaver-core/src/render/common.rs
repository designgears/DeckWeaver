use image::RgbaImage;
use tiny_skia::{Color, FillRule, Mask, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    pub fn as_color(self) -> Color {
        Color::from_rgba8(self.r, self.g, self.b, self.a)
    }

    pub fn invert(self) -> Self {
        Self::new(255 - self.r, 255 - self.g, 255 - self.b, self.a)
    }

    pub fn blend(self, other: Self, amount: f32) -> Self {
        let t = amount.clamp(0.0, 1.0);
        let lerp = |from: u8, to: u8| from as f32 + (to as f32 - from as f32) * t;
        Self::new(
            lerp(self.r, other.r).round() as u8,
            lerp(self.g, other.g).round() as u8,
            lerp(self.b, other.b).round() as u8,
            lerp(self.a, other.a).round() as u8,
        )
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    pub fn luminance(self) -> f32 {
        fn normalize(val: u8) -> f32 {
            let v = val as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * normalize(self.r) + 0.7152 * normalize(self.g) + 0.0722 * normalize(self.b)
    }
}

impl From<(u8, u8, u8)> for Rgba {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Self::rgb(r, g, b)
    }
}

impl From<(u8, u8, u8, u8)> for Rgba {
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self {
        Self::new(r, g, b, a)
    }
}

pub fn wcag_contrast_ratio(a: f32, b: f32) -> f32 {
    let lighter = a.max(b);
    let darker = a.min(b);
    (lighter + 0.05) / (darker + 0.05)
}

const MIN_METER_CONTRAST: f32 = 3.0;
const MAX_METER_BLEND: f32 = 0.65;

pub fn meter_overlay_color(fill: Rgba) -> Rgba {
    let fill_lum = fill.luminance();

    // Fill luminance > 0.30 means even pure white can't achieve 3:1 — blend toward black.
    // Otherwise blend toward white (dark fills).
    let (target, _target_lum) = if fill_lum > 0.30 {
        (COLOR_BLACK, 0.0)
    } else {
        (COLOR_WHITE, 1.0)
    };

    let mut lo = 0.0;
    let mut hi = 1.0;

    for _ in 0..10 {
        let mid = (lo + hi) / 2.0;
        let candidate = fill.blend(target, mid);
        let ratio = wcag_contrast_ratio(fill_lum, candidate.luminance());

        if ratio >= MIN_METER_CONTRAST {
            hi = mid;
        } else {
            lo = mid;
        }
    }

    fill.blend(target, hi.min(MAX_METER_BLEND))
}

pub const COLOR_TRANSPARENT: Rgba = Rgba::new(0, 0, 0, 0);
pub const COLOR_BLACK: Rgba = Rgba::rgb(0, 0, 0);
pub const COLOR_WHITE: Rgba = Rgba::rgb(255, 255, 255);
pub const COLOR_RED: Rgba = Rgba::rgb(255, 0, 0);
/// Fallback accents when PipeWeaver reports no device colour and the user set no override.
/// Also used by the slider renderer.
pub const COLOR_SOURCE_FILL: Rgba = Rgba::rgb(90, 169, 245);
pub const COLOR_TARGET_FILL: Rgba = Rgba::rgb(79, 208, 138);
pub const COLOR_GUTTER_DARK: Rgba = Rgba::rgb(120, 120, 120);
pub const COLOR_GUTTER_LIGHT: Rgba = Rgba::rgb(220, 220, 220);
const GUTTER_LUMINANCE_THRESHOLD: f32 = 0.1;

#[derive(Debug, Clone, Default)]
pub struct RenderParams {
    /// Device name as reported by PipeWeaver. Drawn by the knob renderer; the hosts suppress
    /// their own title overlay for that action.
    pub name: String,
    pub volume: u8,
    pub is_muted: bool,
    pub is_source: bool,
    pub meter_value: u8,
    pub device_color: Option<(u8, u8, u8)>,
    pub volume_bar_color: Option<(u8, u8, u8, u8)>,
    pub meter_color: Option<(u8, u8, u8, u8)>,
    pub meter_invert: bool,
    pub meters_enabled: bool,
    pub mix_b_active: bool,
    pub source_volumes_linked: bool,
    pub mute_profile: u8,
    pub mute_profile_muted: bool,
    /// Draw the volume percentage in the top right of the encoder strip.
    pub show_volume: bool,
}

impl RenderParams {
    pub fn accent_color(&self) -> Rgba {
        self.volume_bar_color
            .map(Rgba::from)
            .or_else(|| self.device_color.map(Rgba::from))
            .unwrap_or(if self.is_source {
                COLOR_SOURCE_FILL
            } else {
                COLOR_TARGET_FILL
            })
    }

    pub fn fill_color(&self) -> Option<Rgba> {
        if self.volume == 0 {
            return None;
        }
        Some(self.accent_color())
    }
}

pub fn gutter_color_for(fill_color: Option<Rgba>) -> Rgba {
    match fill_color {
        Some(c) if c.luminance() < GUTTER_LUMINANCE_THRESHOLD => COLOR_GUTTER_LIGHT,
        _ => COLOR_GUTTER_DARK,
    }
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = radius.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

pub fn solid_paint(color: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color.as_color());
    paint.anti_alias = true;
    paint
}

pub fn fill_background(pixmap: &mut Pixmap, color: Rgba) {
    pixmap.fill(color.as_color());
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Self {
        Self { x, y, w, h, radius }
    }

    pub fn draw_filled(self, pixmap: &mut Pixmap, color: Rgba) {
        self.draw_filled_clipped(pixmap, color, None);
    }

    /// Fill, optionally clipped to a [`Rect::clip_mask`].
    pub fn draw_filled_clipped(self, pixmap: &mut Pixmap, color: Rgba, clip: Option<&Mask>) {
        if let Some(path) = rounded_rect_path(self.x, self.y, self.w, self.h, self.radius) {
            pixmap.fill_path(
                &path,
                &solid_paint(color),
                FillRule::Winding,
                Transform::identity(),
                clip,
            );
        }
    }

    /// Anti-aliased mask of this rect's outline, for clipping content drawn inside it.
    ///
    /// A partial fill is a rounded rect in its own right, and `rounded_rect_path` clamps its
    /// radius to `w / 2` — so once the fill is narrower than the bar is tall, its corners are
    /// squarer than the bar's and poke outside them. Clipping keeps the fill inside the track
    /// at every width.
    pub fn clip_mask(self, width: u32, height: u32) -> Option<Mask> {
        let path = rounded_rect_path(self.x, self.y, self.w, self.h, self.radius)?;
        let mut mask = Mask::new(width, height)?;
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        Some(mask)
    }

    /// Stroke that lies entirely *inside* the rect.
    ///
    /// tiny-skia centres strokes on the path, so a stroked bar ends up half a stroke wider on
    /// every side than an unstroked one built from the same rect — enough to make the volume
    /// bar and the meter lane below it visibly mismatched.
    pub fn draw_inset_stroke(self, pixmap: &mut Pixmap, color: Rgba, width: f32) {
        let half = width * 0.5;
        Rect::new(
            self.x + half,
            self.y + half,
            (self.w - width).max(0.0),
            (self.h - width).max(0.0),
            (self.radius - half).max(0.0),
        )
        .draw_stroked(pixmap, color, width);
    }

    pub fn draw_stroked(self, pixmap: &mut Pixmap, color: Rgba, width: f32) {
        if let Some(path) = rounded_rect_path(self.x, self.y, self.w, self.h, self.radius) {
            let stroke = Stroke {
                width,
                ..Default::default()
            };
            pixmap.stroke_path(
                &path,
                &solid_paint(color),
                &stroke,
                Transform::identity(),
                None,
            );
        }
    }
}

fn stroke_line(pixmap: &mut Pixmap, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Rgba) {
    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    if let Some(path) = pb.finish() {
        let stroke = Stroke {
            width,
            line_cap: tiny_skia::LineCap::Round,
            ..Default::default()
        };
        pixmap.stroke_path(
            &path,
            &solid_paint(color),
            &stroke,
            Transform::identity(),
            None,
        );
    }
}

pub fn draw_symbol(
    pixmap: &mut Pixmap,
    cx: f32,
    cy: f32,
    size: f32,
    width: f32,
    color: Rgba,
    is_plus: bool,
) {
    let half = size / 2.0;
    stroke_line(pixmap, cx - half, cy, cx + half, cy, width, color);
    if is_plus {
        stroke_line(pixmap, cx, cy - half, cx, cy + half, width, color);
    }
}

pub fn draw_diagonal_line(
    pixmap: &mut Pixmap,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    width: f32,
    color: Rgba,
) {
    stroke_line(pixmap, x1, y1, x2, y2, width, color);
}

pub fn create_unavailable_pixmap(width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    fill_background(&mut pixmap, COLOR_TRANSPARENT);

    let min_side = width.min(height) as f32;
    let inset = (min_side * 0.22).max(8.0);
    let stroke_width = (min_side * 0.12).max(4.0);
    let w = width as f32;
    let h = height as f32;

    draw_diagonal_line(
        &mut pixmap,
        inset,
        inset,
        w - inset,
        h - inset,
        stroke_width,
        COLOR_RED,
    );
    draw_diagonal_line(
        &mut pixmap,
        w - inset,
        inset,
        inset,
        h - inset,
        stroke_width,
        COLOR_RED,
    );

    Some(pixmap)
}

/// Convert Pixmap to raw RGBA bytes (no PNG encoding - much faster!)
pub fn pixmap_to_rgba(pixmap: &Pixmap) -> Option<(Vec<u8>, u32, u32)> {
    let data = pixmap.data();
    Some((data.to_vec(), pixmap.width(), pixmap.height()))
}

pub fn create_filled_pixmap(width: u32, height: u32, color: Rgba) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    fill_background(&mut pixmap, color);
    Some(pixmap)
}

/// Source-over blit of an RGBA image onto `pixmap`.
///
/// Shared by the icon compositing in every renderer and by the text pipeline; previously each
/// renderer carried its own copy of this loop.
pub fn blit_rgba8(pixmap: &mut Pixmap, rgba: &RgbaImage, dest_x: i32, dest_y: i32) {
    blit(pixmap, rgba, dest_x, dest_y, None);
}

/// Source-over blit of `rgba`'s *coverage* tinted with `color`.
///
/// The source RGB is ignored and its alpha is scaled by `color.a`, so one rasterised mask can
/// be drawn as a shadow, a halo and the glyphs themselves without re-rasterising.
pub fn blit_alpha_tinted(pixmap: &mut Pixmap, rgba: &RgbaImage, dest_x: i32, dest_y: i32, color: Rgba) {
    if color.a == 0 {
        return;
    }
    blit(pixmap, rgba, dest_x, dest_y, Some(color));
}

fn blit(pixmap: &mut Pixmap, rgba: &RgbaImage, dest_x: i32, dest_y: i32, tint: Option<Rgba>) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let tint_scale = tint.map_or(1.0, |c| c.a as f32 / 255.0);

    for (ix, iy, pixel) in rgba.enumerate_pixels() {
        let px = dest_x + ix as i32;
        let py = dest_y + iy as i32;
        if px < 0 || py < 0 || px >= pw || py >= ph {
            continue;
        }

        let src_a = (pixel[3] as f32 / 255.0) * tint_scale;
        if src_a <= 0.0 {
            continue;
        }
        let (src_r, src_g, src_b) = match tint {
            Some(c) => (c.r, c.g, c.b),
            None => (pixel[0], pixel[1], pixel[2]),
        };

        let idx = ((py as u32 * pixmap.width() + px as u32) * 4) as usize;
        let data = pixmap.data_mut();
        let dst_r = data[idx] as f32;
        let dst_g = data[idx + 1] as f32;
        let dst_b = data[idx + 2] as f32;
        let dst_a = data[idx + 3] as f32 / 255.0;
        let out_a = src_a + dst_a * (1.0 - src_a);

        let blend = |src: u8, dst: f32| -> u8 {
            if out_a <= 0.0 {
                0
            } else {
                (((src as f32 * src_a) + (dst * dst_a * (1.0 - src_a))) / out_a).round() as u8
            }
        };

        data[idx] = blend(src_r, dst_r);
        data[idx + 1] = blend(src_g, dst_g);
        data[idx + 2] = blend(src_b, dst_b);
        data[idx + 3] = (out_a * 255.0).round() as u8;
    }
}

/// A dark halo drawn from a coverage mask so white content survives a light or busy background.
/// This is what replaces the opaque panel the strip used to paint.
#[derive(Debug, Clone, Copy)]
pub struct OutlineSpec {
    /// Applied once per offset in [`HALO_DISC`]. Overlapping blits accumulate, so the halo is
    /// dense against the glyph edge and falls off naturally outwards — a single thin ring is
    /// not enough to separate white text from a white background.
    pub halo: Rgba,
    pub drop: Rgba,
    pub drop_offset: (i32, i32),
}

/// Every integer offset inside a radius-2 disc, excluding the centre.
const HALO_DISC: [(i32, i32); 12] = [
    (0, -2),
    (0, -1),
    (0, 1),
    (0, 2),
    (-2, 0),
    (-1, 0),
    (1, 0),
    (2, 0),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// Draw `spec` around `mask`'s coverage. Call before blitting the content itself.
pub fn blit_outline(pixmap: &mut Pixmap, mask: &RgbaImage, x: i32, y: i32, spec: &OutlineSpec) {
    let (dx, dy) = spec.drop_offset;
    blit_alpha_tinted(pixmap, mask, x + dx, y + dy, spec.drop);
    for (dx, dy) in HALO_DISC {
        blit_alpha_tinted(pixmap, mask, x + dx, y + dy, spec.halo);
    }
}

/// Aspect-fit `png_data` into a `max_size` box, never upscaling. Returns the decoded RGBA and
/// its scaled dimensions.
pub fn decode_icon(png_data: &[u8], max_size: f32) -> Option<(RgbaImage, u32, u32)> {
    let img = image::load_from_memory(png_data).ok()?;
    let (iw, ih) = (img.width() as f32, img.height() as f32);
    let scale = (max_size / iw).min(max_size / ih).min(1.0);
    let (sw, sh) = ((iw * scale) as u32, (ih * scale) as u32);

    let resized = img
        .resize(sw, sh, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Some((resized, sw, sh))
}
