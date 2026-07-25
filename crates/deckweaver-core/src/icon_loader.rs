use image::{imageops::FilterType, ImageEncoder};
use resvg::{tiny_skia, usvg};
use std::fs;
use std::path::Path;

const DEFAULT_ICON_SIZE: u32 = 200;
const MIN_ICON_SIZE: u32 = 200;

pub fn load_icon_to_png_bytes(path: &str) -> Option<Vec<u8>> {
    let path_obj = Path::new(path);

    if !path_obj.exists() {
        return None;
    }

    if path_obj
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("svg"))
        .unwrap_or(false)
    {
        return load_svg_to_png(path);
    }

    load_image_to_png(path)
}

pub fn svg_data_to_png_bytes(svg_data: &[u8]) -> Option<Vec<u8>> {
    let opt = usvg::Options::default();
    let tree = match usvg::Tree::from_data(svg_data, &opt) {
        Ok(tree) => tree,
        Err(e) => {
            tracing::warn!("Failed to parse SVG data: {}", e);
            return None;
        }
    };

    let size = tree.size();
    let (target_width, target_height, scale_x, scale_y) =
        if size.width() > 0.0 && size.height() > 0.0 {
            let max_dim = size.width().max(size.height());
            let scale = DEFAULT_ICON_SIZE as f32 / max_dim;
            let tw = (size.width() * scale) as u32;
            let th = (size.height() * scale) as u32;
            let sx = tw as f32 / size.width();
            let sy = th as f32 / size.height();
            (tw, th, sx, sy)
        } else {
            (DEFAULT_ICON_SIZE, DEFAULT_ICON_SIZE, 1.0, 1.0)
        };

    let mut pixmap = match tiny_skia::Pixmap::new(target_width, target_height) {
        Some(pixmap) => pixmap,
        None => {
            tracing::warn!("Failed to create pixmap for SVG");
            return None;
        }
    };

    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    match pixmap.encode_png() {
        Ok(png_data) => Some(png_data),
        Err(e) => {
            tracing::warn!("Failed to encode SVG as PNG: {}", e);
            None
        }
    }
}

fn load_svg_to_png(path: &str) -> Option<Vec<u8>> {
    let svg_data = match fs::read(path) {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!("Failed to read SVG file {}: {}", path, e);
            return None;
        }
    };
    svg_data_to_png_bytes(&svg_data)
}

fn load_image_to_png(path: &str) -> Option<Vec<u8>> {
    let img = match image::open(path) {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!("Failed to load image {}: {}", path, e);
            return None;
        }
    };

    let rgba_img = img.to_rgba8();
    let (width, height) = rgba_img.dimensions();
    let max_dim = width.max(height);

    let final_img = if max_dim < MIN_ICON_SIZE {
        let scale = MIN_ICON_SIZE as f32 / max_dim as f32;
        let new_width = (width as f32 * scale) as u32;
        let new_height = (height as f32 * scale) as u32;
        image::imageops::resize(&rgba_img, new_width, new_height, FilterType::Triangle)
    } else {
        rgba_img
    };

    let mut png_data = Vec::new();
    {
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        if let Err(e) = encoder.write_image(
            &final_img,
            final_img.width(),
            final_img.height(),
            image::ColorType::Rgba8.into(),
        ) {
            tracing::warn!("Failed to encode image as PNG {}: {}", path, e);
            return None;
        }
    }

    Some(png_data)
}
