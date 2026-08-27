//! Slider keys — the same capsule-and-recessed-meter language as the encoder strip, rotated
//! upright and split across two keys.
//!
//! The bar is drawn once at double height, then the top or bottom square is cropped out, so a
//! pair of stacked keys reads as one continuous fader. `orientation = "horizontal"` rotates the
//! crop a quarter turn.

use super::common::*;
use super::text::{draw_text, truncate_to_width, TextAlign, TextStyle};
use super::theme;
use tiny_skia::Pixmap;

pub struct SliderRenderer {
    button_size: u32,
}

impl SliderRenderer {
    pub fn new(button_size: u32) -> Self {
        Self { button_size }
    }

    pub fn render_internal_png(
        &self,
        params: &RenderParams,
        is_top: bool,
        is_horizontal: bool,
        cached_icon: Option<&crate::action::CachedIcon>,
    ) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&self.render_internal(params, is_top, is_horizontal, cached_icon)?)
    }

    /// Dimmed placeholder with a reason, used instead of the fault cross when an app action simply
    /// has nothing playing to control.
    pub fn render_idle_internal(&self, message: &str) -> Option<(Vec<u8>, u32, u32)> {
        let mut pixmap = create_filled_pixmap(self.button_size, self.button_size, theme::IDLE_BG)?;
        let width = self.button_size as f32 - theme::PAD * 2.0;
        let label = truncate_to_width(message, width, theme::LABEL_SIZE);
        draw_text(
            &mut pixmap,
            &label,
            Rect::new(
                theme::PAD,
                (self.button_size as f32 - theme::LABEL_SIZE) * 0.5,
                width,
                theme::LABEL_SIZE * 1.4,
                0.0,
            ),
            &TextStyle::new(theme::LABEL_SIZE, theme::TEXT_IDLE, TextAlign::Center),
        );
        pixmap_to_rgba(&pixmap)
    }

    pub fn render_unavailable_internal(&self) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&create_unavailable_pixmap(
            self.button_size,
            self.button_size,
        )?)
    }

    pub fn render_loading_internal(&self) -> Option<(Vec<u8>, u32, u32)> {
        let params = RenderParams::default();
        pixmap_to_rgba(&self.render_internal(&params, true, false, None)?)
    }

    fn render_internal(
        &self,
        params: &RenderParams,
        is_top: bool,
        is_horizontal: bool,
        cached_icon: Option<&crate::action::CachedIcon>,
    ) -> Option<Pixmap> {
        let mut full = Pixmap::new(self.button_size, self.button_size * 2)?;
        fill_background(&mut full, COLOR_TRANSPARENT);

        // The app's art as a faded backdrop under the whole fader. Pre-cropped to cover the
        // double-height stack (`IconSizing::Cover`), so a stacked pair of keys shows the two
        // halves of one continuous image, the same way the bar splits.
        if let Some(icon) = cached_icon {
            let x = (self.button_size as i32 - icon.width as i32) / 2;
            let y = (self.button_size as i32 * 2 - icon.height as i32) / 2;
            blit_rgba8(&mut full, &icon.rgba8, x, y);
        }

        self.draw_slider_stack(&mut full, params);
        self.render_meter_overlay(&mut full, params);

        let square = self.extract_square(&full, is_top)?;

        if is_horizontal {
            self.rotate_cw(&square)
        } else {
            Some(square)
        }
    }

    // -- layout -------------------------------------------------------------

    /// The bar, in the coordinates of the double-height stack.
    fn bar_bounds(&self) -> (f32, f32, f32, f32) {
        let size = self.button_size as f32;
        let width = size * theme::SLIDER_BAR_WIDTH_RATIO;
        let inset = size * theme::SLIDER_END_INSET_RATIO;
        (
            (size - width) * 0.5,
            inset,
            width,
            (size * 2.0 - inset * 2.0).max(0.0),
        )
    }

    /// The meter lane, recessed into the bar and horizontally centred in it.
    fn meter_bounds(&self) -> (f32, f32, f32, f32) {
        let (bar_x, bar_y, bar_w, bar_h) = self.bar_bounds();
        let lane_w = bar_w * theme::SLIDER_METER_WIDTH_RATIO;
        let inset = bar_w * theme::SLIDER_METER_INSET_RATIO;
        (
            bar_x + (bar_w - lane_w) * 0.5,
            bar_y + inset,
            lane_w,
            (bar_h - inset * 2.0).max(0.0),
        )
    }

    // -- drawing ------------------------------------------------------------

    fn draw_slider_stack(&self, pixmap: &mut Pixmap, params: &RenderParams) {
        let (x, y, w, h) = self.bar_bounds();
        let radius = w * 0.5;
        let track = Rect::new(x, y, w, h, radius);

        track.draw_filled(pixmap, theme::BAR_TRACK);

        let fill_h = (params.volume as f32 / 100.0) * h;
        if fill_h > 0.0 {
            // Clipped to the track: a fill shorter than the bar is wide would otherwise get a
            // radius clamped to its own half-height and poke out of the rounded end.
            let clip = track.clip_mask(pixmap.width(), pixmap.height());
            Rect::new(x, y + h - fill_h, w, fill_h, radius).draw_filled_clipped(
                pixmap,
                params.accent_color(),
                clip.as_ref(),
            );
        }

        track.draw_inset_stroke(pixmap, theme::BAR_EDGE, theme::BAR_EDGE_WIDTH);
    }

    /// The meter lane, growing from the bottom like the fill.
    fn render_meter_overlay(&self, pixmap: &mut Pixmap, params: &RenderParams) {
        if !params.meters_enabled || params.meter_value == 0 {
            return;
        }

        let (x, y, w, h) = self.meter_bounds();
        let fill_h = (params.meter_value as f32 / 100.0) * h;
        if fill_h <= 0.0 {
            return;
        }

        // Ring first, then the lane on top, so the meter stays legible where it crosses from
        // the accent fill onto the darker track above it.
        let edge = theme::METER_EDGE_WIDTH;
        let ring_w = w + edge * 2.0;
        Rect::new(
            x - edge,
            y + h - fill_h - edge,
            ring_w,
            fill_h + edge * 2.0,
            ring_w * 0.5,
        )
        .draw_filled(pixmap, theme::METER_EDGE);
        Rect::new(x, y + h - fill_h, w, fill_h, w * 0.5)
            .draw_filled(pixmap, params.meter_fill_color());
    }

    fn rotate_cw(&self, pixmap: &Pixmap) -> Option<Pixmap> {
        let (w, h) = (pixmap.width(), pixmap.height());
        let mut rotated = Pixmap::new(h, w)?;
        let (src, dst) = (pixmap.data(), rotated.data_mut());

        for y in 0..h {
            for x in 0..w {
                let si = ((y * w + x) * 4) as usize;
                let di = ((x * h + (h - 1 - y)) * 4) as usize;
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
        Some(rotated)
    }

    fn extract_square(&self, pixmap: &Pixmap, is_top: bool) -> Option<Pixmap> {
        let mut result = Pixmap::new(self.button_size, self.button_size)?;
        let y_off = if is_top { 0 } else { self.button_size as usize };
        let row_bytes = self.button_size as usize * 4;

        for y in 0..self.button_size as usize {
            let src = (y + y_off) * row_bytes;
            let dst = y * row_bytes;
            result.data_mut()[dst..dst + row_bytes]
                .copy_from_slice(&pixmap.data()[src..src + row_bytes]);
        }

        Some(result)
    }
}
