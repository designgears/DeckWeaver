//! Stream Deck+ encoder strip (200x100).
//!
//! Everything is drawn onto a transparent pixmap so the user's own background shows through —
//! there is deliberately no panel or scrim here. Contrast on light or busy backgrounds comes
//! from the text shadow/halo in [`super::text`] and the hairline edges on the bars.

use super::common::*;
use super::text::{draw_text, truncate_to_width, TextAlign, TextStyle};
use super::theme;
use tiny_skia::Pixmap;

pub struct KnobRenderer {
    width: u32,
    height: u32,
}

/// A dark capsule with centred text, anchored at its left edge. Every status indicator uses one
/// so they read as a single control language, and so the 12px labels never have to survive on
/// outline alone over a light background.
fn draw_chip(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, label: &str, fill: Rgba, text: Rgba) {
    let chip = Rect::new(x, y, w, theme::CHIP_H, theme::CHIP_RADIUS);
    chip.draw_filled(pixmap, fill);
    chip.draw_inset_stroke(pixmap, theme::BAR_EDGE, theme::CHIP_EDGE_WIDTH);
    draw_text(
        pixmap,
        label,
        chip,
        &TextStyle::new(theme::LABEL_SIZE, text, TextAlign::Center).without_outline(),
    );
}

impl KnobRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn render_internal_png(
        &self,
        params: &RenderParams,
        icon_png: Option<Vec<u8>>,
    ) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&self.render_internal(params, icon_png, None)?)
    }

    pub fn render_internal_png_with_cached(
        &self,
        params: &RenderParams,
        cached_icon: Option<&crate::action::CachedIcon>,
    ) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&self.render_internal(params, None, cached_icon)?)
    }

    pub fn render_unavailable_internal(&self) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&create_unavailable_pixmap(self.width, self.height)?)
    }

    pub fn render_loading_internal(&self) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&create_filled_pixmap(
            self.width,
            self.height,
            COLOR_TRANSPARENT,
        )?)
    }

    fn render_internal(
        &self,
        params: &RenderParams,
        icon_png: Option<Vec<u8>>,
        cached_icon: Option<&crate::action::CachedIcon>,
    ) -> Option<Pixmap> {
        let mut pixmap = self.render_base(params, icon_png, cached_icon)?;

        if params.meters_enabled && params.meter_value > 0 {
            self.render_meter_overlay(&mut pixmap, params);
        }

        Some(pixmap)
    }

    /// Everything except the meter fill. Cached by the render loop and cloned per frame, so it
    /// must not depend on the meter value.
    pub fn render_base(
        &self,
        params: &RenderParams,
        icon_png: Option<Vec<u8>>,
        cached_icon: Option<&crate::action::CachedIcon>,
    ) -> Option<Pixmap> {
        let mut pixmap = Pixmap::new(self.width, self.height)?;
        fill_background(&mut pixmap, COLOR_TRANSPARENT);

        self.draw_icon(&mut pixmap, icon_png, cached_icon);
        self.draw_header(&mut pixmap, params);
        self.draw_volume_bar(&mut pixmap, params);
        self.draw_status_row(&mut pixmap, params);

        Some(pixmap)
    }

    /// The meter fill only — drawn on top of a clone of the cached base every frame.
    pub fn render_meter_overlay(&self, pixmap: &mut Pixmap, params: &RenderParams) {
        if !params.meters_enabled || params.meter_value == 0 {
            return;
        }

        let (x, y, w, h) = self.meter_bounds();
        let fill_w = (params.meter_value as f32 / 100.0) * w;
        if fill_w <= 0.0 {
            return;
        }

        // Ring first, then the lane on top: it hugs the meter so the rest of the volume fill
        // stays untouched.
        let edge = theme::METER_EDGE_WIDTH;
        let ring_h = h + edge * 2.0;
        Rect::new(x - edge, y - edge, fill_w + edge * 2.0, ring_h, ring_h * 0.5)
            .draw_filled(pixmap, theme::METER_EDGE);
        Rect::new(x, y, fill_w, h, h * 0.5).draw_filled(pixmap, params.meter_fill_color());
    }

    // -- layout helpers -----------------------------------------------------

    fn bar_bounds(&self) -> (f32, f32, f32, f32) {
        (
            theme::PAD,
            theme::BAR_Y,
            self.width as f32 - theme::PAD * 2.0,
            theme::BAR_H,
        )
    }

    /// The meter lane, inset into the volume bar and vertically centred in it.
    fn meter_bounds(&self) -> (f32, f32, f32, f32) {
        let (bar_x, bar_y, bar_w, bar_h) = self.bar_bounds();
        (
            bar_x + theme::METER_INSET_X,
            bar_y + (bar_h - theme::METER_H) * 0.5,
            (bar_w - theme::METER_INSET_X * 2.0).max(0.0),
            theme::METER_H,
        )
    }

    // -- drawing ------------------------------------------------------------

    fn draw_icon(
        &self,
        pixmap: &mut Pixmap,
        icon_png: Option<Vec<u8>>,
        cached_icon: Option<&crate::action::CachedIcon>,
    ) {
        if let Some(cached) = cached_icon {
            self.blit_icon(pixmap, &cached.rgba8, cached.width, cached.height);
        } else if let Some(png) = icon_png {
            if let Some((rgba, sw, sh)) = decode_icon(&png, theme::ICON_SIZE) {
                self.blit_icon(pixmap, &rgba, sw, sh);
            }
        }
    }

    fn blit_icon(&self, pixmap: &mut Pixmap, rgba: &image::RgbaImage, sw: u32, sh: u32) {
        // Centre within the icon box so differently-proportioned icons stay put.
        let x = (theme::PAD + (theme::ICON_SIZE - sw as f32) * 0.5).round() as i32;
        let y = (theme::ICON_Y + (theme::ICON_SIZE - sh as f32) * 0.5).round() as i32;

        // Icons are typically white glyphs, so they need the same surround as the text.
        blit_outline(pixmap, rgba, x, y, &theme::CONTENT_OUTLINE);
        blit_rgba8(pixmap, rgba, x, y);
    }

    fn draw_header(&self, pixmap: &mut Pixmap, params: &RenderParams) {
        let width = self.width as f32;
        let text_color = if params.mute_profile_muted {
            theme::TEXT_DIMMED
        } else {
            theme::TEXT_PRIMARY
        };

        // With the readout hidden the name simply takes the space back.
        let name_right = if params.show_volume {
            width - theme::PAD - theme::VOLUME_SLOT_W - theme::NAME_GAP
        } else {
            width - theme::PAD
        };
        let name_w = (name_right - theme::NAME_X).max(0.0);

        let name = truncate_to_width(&params.name, name_w, theme::NAME_SIZE);
        draw_text(
            pixmap,
            &name,
            Rect::new(theme::NAME_X, theme::NAME_Y, name_w, theme::NAME_H, 0.0),
            &TextStyle::new(theme::NAME_SIZE, text_color, TextAlign::Left),
        );

        if params.show_volume {
            draw_text(
                pixmap,
                &format!("{}%", params.volume),
                Rect::new(
                    width - theme::PAD - theme::VOLUME_SLOT_W,
                    theme::NAME_Y,
                    theme::VOLUME_SLOT_W,
                    theme::NAME_H,
                    0.0,
                ),
                &TextStyle::new(theme::VOLUME_SIZE, text_color, TextAlign::Right),
            );
        }
    }

    fn draw_volume_bar(&self, pixmap: &mut Pixmap, params: &RenderParams) {
        let (x, y, w, h) = self.bar_bounds();
        let radius = h * 0.5;
        let track = Rect::new(x, y, w, h, radius);

        track.draw_filled(pixmap, theme::BAR_TRACK);

        let fill_w = (params.volume as f32 / 100.0) * w;
        if fill_w > 0.0 {
            let mut color = params.accent_color();
            if params.mute_profile_muted {
                color = color.with_alpha(theme::MUTED_FILL_ALPHA);
            }
            let clip = track.clip_mask(self.width, self.height);
            Rect::new(x, y, fill_w, h, radius).draw_filled_clipped(pixmap, color, clip.as_ref());
        }

        track.draw_inset_stroke(pixmap, theme::BAR_EDGE, theme::BAR_EDGE_WIDTH);
    }

    fn draw_status_row(&self, pixmap: &mut Pixmap, params: &RenderParams) {
        // An app stream has no mute profiles, no A/B mix and no volume linking, so those chips
        // would be three pieces of dead furniture. Show where the app is routed instead — the one
        // piece of routing state that is both real and not visible anywhere else on the strip.
        if let Some(channel) = params.routed_to.as_deref() {
            let width = self.width as f32 - theme::PAD * 2.0;
            let label = truncate_to_width(channel, theme::LABEL_SIZE, width - theme::PAD * 2.0);
            draw_chip(
                pixmap,
                theme::PAD,
                theme::CHIP_Y_BOTTOM,
                width,
                &label,
                theme::CHIP_BG,
                theme::TEXT_SECONDARY,
            );
            return;
        }

        // The mute chip keeps the same slot in both states, so the label doesn't shift when you
        // mute — only the fill changes.
        let (fill, text) = if params.mute_profile_muted {
            (theme::MUTE, theme::TEXT_PRIMARY)
        } else {
            (theme::CHIP_BG, theme::TEXT_SECONDARY)
        };
        draw_chip(
            pixmap,
            theme::PAD,
            theme::CHIP_Y_BOTTOM,
            theme::MUTE_CHIP_W,
            &format!("Mute {}", params.mute_profile + 1),
            fill,
            text,
        );

        let (mix_label, mix_color) = if params.mix_b_active {
            ("Mix B", theme::MIX_B)
        } else {
            ("Mix A", theme::MIX_A)
        };
        draw_chip(
            pixmap,
            (self.width as f32 - theme::MIX_CHIP_W) * 0.5,
            theme::CHIP_Y_BOTTOM,
            theme::MIX_CHIP_W,
            mix_label,
            theme::CHIP_BG,
            mix_color,
        );

        if params.is_source && params.source_volumes_linked {
            draw_chip(
                pixmap,
                self.width as f32 - theme::PAD - theme::LINK_CHIP_W,
                theme::CHIP_Y_BOTTOM,
                theme::LINK_CHIP_W,
                "Linked",
                theme::CHIP_BG,
                theme::TEXT_SECONDARY,
            );
        }
    }
}
