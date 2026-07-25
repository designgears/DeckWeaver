use super::common::*;
use tiny_skia::Pixmap;

const CORNER_INSET: f32 = 16.0;
const BAR_WIDTH: f32 = 25.0;
const BAR_OFFSET_Y: f32 = 0.0;
const STROKE_WIDTH: f32 = 2.0;

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
    ) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&self.render_internal(params, is_top, is_horizontal)?)
    }

    pub fn render_unavailable_internal(&self) -> Option<(Vec<u8>, u32, u32)> {
        pixmap_to_rgba(&create_unavailable_pixmap(
            self.button_size,
            self.button_size,
        )?)
    }

    pub fn render_loading_internal(&self) -> Option<(Vec<u8>, u32, u32)> {
        let params = RenderParams {
            name: String::new(),
            volume: 0,
            is_muted: false,
            is_source: false,
            meter_value: 0,
            device_color: None,
            volume_bar_color: None,
            meter_color: None,
            meter_invert: true,
            meters_enabled: false,
            mix_b_active: false,
            source_volumes_linked: false,
            mute_profile: 0,
            mute_profile_muted: false,
            show_volume: false,
        };
        pixmap_to_rgba(&self.render_internal(&params, true, false)?)
    }

    fn render_internal(
        &self,
        params: &RenderParams,
        is_top: bool,
        is_horizontal: bool,
    ) -> Option<Pixmap> {
        let mut full = Pixmap::new(self.button_size, self.button_size * 2)?;
        fill_background(&mut full, COLOR_TRANSPARENT);

        self.draw_slider_stack(&mut full, params);

        if params.meters_enabled && params.meter_value > 0 {
            self.render_meter_overlay(&mut full, params);
        }

        let square = self.extract_square(&full, is_top)?;

        if is_horizontal {
            Some(self.rotate_cw(&square)?)
        } else {
            Some(square)
        }
    }

    fn draw_slider_stack(&self, pixmap: &mut Pixmap, params: &RenderParams) {
        let size = self.button_size as f32;
        let double_h = size * 2.0;
        let slider_y = CORNER_INSET + BAR_OFFSET_Y;
        let slider_h = double_h - CORNER_INSET * 2.0 - BAR_OFFSET_Y;
        let slider_x = (size - BAR_WIDTH) / 2.0;

        let fill_color = params.fill_color();
        let fill_h = (params.volume as f32 / 100.0) * slider_h;
        let bar = Rect::new(slider_x, slider_y, BAR_WIDTH, slider_h, 0.0);
        bar.draw_filled(pixmap, gutter_color_for(fill_color));

        if let Some(color) = fill_color {
            if fill_h > 0.0 {
                Rect::new(
                    slider_x,
                    slider_y + slider_h - fill_h,
                    BAR_WIDTH,
                    fill_h,
                    0.0,
                )
                .draw_filled(pixmap, color);
            }
        }

        bar.draw_stroked(pixmap, COLOR_BLACK, STROKE_WIDTH);
    }

    fn render_meter_overlay(&self, full: &mut Pixmap, params: &RenderParams) {
        let size = self.button_size as f32;
        let double_h = size * 2.0;
        let slider_y = CORNER_INSET + BAR_OFFSET_Y;
        let slider_h = double_h - CORNER_INSET * 2.0 - BAR_OFFSET_Y;
        let slider_x = (size - BAR_WIDTH) / 2.0;
        let inset = STROKE_WIDTH * 0.5;
        let inner_x = slider_x + inset;
        let inner_w = (BAR_WIDTH - inset * 2.0).max(0.0);

        let fill_color = params.fill_color();
        let fill_h = (params.volume as f32 / 100.0) * slider_h;

        if params.meter_value > 0 && fill_h > 0.0 {
            if let Some(fc) = fill_color {
                let fill_y = slider_y + slider_h - fill_h;
                let available = (fill_h - inset * 2.0).max(0.0);
                if available > 0.0 {
                    let meter_h = (params.meter_value as f32 / 100.0) * available;
                    let meter_y = fill_y + inset + available - meter_h;
                    let meter_color = meter_overlay_color(fc);
                    Rect::new(inner_x, meter_y, inner_w, meter_h, 0.0)
                        .draw_filled(full, meter_color);
                }
            }
        }
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
