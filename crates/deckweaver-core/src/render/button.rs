use super::common::*;
use tiny_skia::Pixmap;

const LARGE_SYMBOL_RATIO: f32 = 0.5;
const LARGE_LINE_WIDTH_RATIO: f32 = 0.13;
const SMALL_SYMBOL_RATIO: f32 = 0.12;
const SMALL_LINE_WIDTH_RATIO: f32 = 0.04;
const CORNER_INSET_RATIO: f32 = 0.08;
const ICON_INSET_RATIO: f32 = 0.25;
const MIN_LINE_WIDTH: f32 = 2.0;
const MIN_CORNER_INSET: f32 = 4.0;

pub struct ButtonRenderer {
    button_size: u32,
}

impl ButtonRenderer {
    pub fn new(button_size: u32) -> Self {
        Self { button_size }
    }

    pub fn render_internal_png(
        &self,
        is_plus: Option<bool>,
        icon_png: Option<Vec<u8>>,
        is_muted: bool,
        show_overlay: bool,
    ) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&self.render_internal(is_plus, icon_png, None, is_muted, show_overlay)?)
    }

    pub fn render_internal_png_with_cached(
        &self,
        is_plus: Option<bool>,
        cached_icon: Option<&crate::action::CachedIcon>,
        is_muted: bool,
        show_overlay: bool,
    ) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&self.render_internal(is_plus, None, cached_icon, is_muted, show_overlay)?)
    }

    pub fn render_unavailable_internal(&self) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&create_unavailable_pixmap(
            self.button_size,
            self.button_size,
        )?)
    }

    pub fn render_loading_internal(&self) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&create_filled_pixmap(
            self.button_size,
            self.button_size,
            COLOR_TRANSPARENT,
        )?)
    }

    fn render_internal(
        &self,
        is_plus: Option<bool>,
        icon_png: Option<Vec<u8>>,
        cached_icon: Option<&crate::action::CachedIcon>,
        is_muted: bool,
        show_overlay: bool,
    ) -> Option<Pixmap> {
        let size = self.button_size as f32;
        let mut pixmap = Pixmap::new(self.button_size, self.button_size)?;
        fill_background(&mut pixmap, COLOR_TRANSPARENT);

        let has_icon = cached_icon.is_some() || icon_png.is_some();

        if let Some(cached) = cached_icon {
            self.composite_rgba8(&mut pixmap, &cached.rgba8, cached.width, cached.height);
        } else if let Some(png_data) = icon_png {
            self.composite_icon(&mut pixmap, &png_data);
        }

        if show_overlay {
            match is_plus {
                Some(is_plus) => {
                    let (cx, cy, sym_size, line_width) = self.symbol_layout(size, has_icon);
                    draw_symbol(
                        &mut pixmap,
                        cx,
                        cy,
                        sym_size,
                        line_width,
                        COLOR_WHITE,
                        is_plus,
                    );
                }
                None => {
                    if has_icon {
                        if is_muted {
                            self.draw_icon_mute_slash(&mut pixmap, size);
                        }
                        self.draw_corner_toggle_hint(&mut pixmap, size);
                    } else if is_muted {
                        self.draw_center_mute_slash(&mut pixmap, size);
                    }
                }
            }
        }

        Some(pixmap)
    }

    fn symbol_layout(&self, size: f32, has_icon: bool) -> (f32, f32, f32, f32) {
        if has_icon {
            let inset = (size * CORNER_INSET_RATIO).max(MIN_CORNER_INSET);
            let sym = size * SMALL_SYMBOL_RATIO;
            (
                size - inset - sym / 2.0,
                size - inset - sym / 2.0,
                sym,
                (size * SMALL_LINE_WIDTH_RATIO).max(MIN_LINE_WIDTH),
            )
        } else {
            let center = size / 2.0;
            (
                center,
                center,
                size * LARGE_SYMBOL_RATIO,
                (size * LARGE_LINE_WIDTH_RATIO).max(3.0),
            )
        }
    }

    fn draw_icon_mute_slash(&self, pixmap: &mut Pixmap, size: f32) {
        let inset = (size * ICON_INSET_RATIO).max(MIN_CORNER_INSET);
        let icon_size = size - inset * 2.0;
        draw_diagonal_line(
            pixmap,
            inset,
            inset,
            inset + icon_size,
            inset + icon_size,
            6.0,
            COLOR_RED,
        );
    }

    fn draw_center_mute_slash(&self, pixmap: &mut Pixmap, size: f32) {
        let center = size / 2.0;
        let sym_size = size * LARGE_SYMBOL_RATIO;
        let offset = sym_size * 0.35;
        draw_diagonal_line(
            pixmap,
            center - offset,
            center - offset,
            center + offset,
            center + offset,
            6.0,
            COLOR_RED,
        );
    }

    fn draw_corner_toggle_hint(&self, pixmap: &mut Pixmap, size: f32) {
        let inset = (size * CORNER_INSET_RATIO).max(MIN_CORNER_INSET);
        let corner_sym = size * SMALL_SYMBOL_RATIO;
        let corner_cx = size - inset - corner_sym / 2.0;
        let corner_cy = size - inset - corner_sym / 2.0;
        let corner_width = (size * SMALL_LINE_WIDTH_RATIO).max(MIN_LINE_WIDTH);
        let corner_offset = corner_sym * 0.35;
        draw_diagonal_line(
            pixmap,
            corner_cx + corner_offset,
            corner_cy - corner_offset,
            corner_cx - corner_offset,
            corner_cy + corner_offset,
            corner_width,
            COLOR_WHITE,
        );
    }

    fn composite_icon(&self, pixmap: &mut Pixmap, png_data: &[u8]) {
        let size = self.button_size as f32;
        let inset = (size * ICON_INSET_RATIO).max(MIN_CORNER_INSET);

        if let Some((rgba, sw, sh)) = decode_icon(png_data, size - inset * 2.0) {
            self.composite_rgba8(pixmap, &rgba, sw, sh);
        }
    }

    fn composite_rgba8(&self, pixmap: &mut Pixmap, rgba8: &image::RgbaImage, sw: u32, sh: u32) {
        let size = self.button_size as f32;
        blit_rgba8(
            pixmap,
            rgba8,
            ((size - sw as f32) / 2.0) as i32,
            ((size - sh as f32) / 2.0) as i32,
        );
    }
}
