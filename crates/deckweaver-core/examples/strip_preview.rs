//! Renders the Stream Deck+ encoder strip and the slider keys across a matrix of device
//! states, composited over four backgrounds, into contact sheets under `target/preview/`.
//!
//! The strip is drawn onto transparency so the user's own background shows through — the white
//! and checkerboard columns are the ones that prove the text shadows and bar edges are doing
//! their job.
//!
//! ```sh
//! cargo run -p deckweaver-core --example strip_preview
//! ```

use deckweaver_core::{
    load_icon_to_png_bytes, KnobRenderer, RenderParams, SliderRenderer, ENCODER_STRIP_HEIGHT,
    ENCODER_STRIP_WIDTH, KEYPAD_SIZE,
};
use image::{Rgba, RgbaImage};

const GUTTER: u32 = 10;

fn main() {
    let width = ENCODER_STRIP_WIDTH;
    let height = ENCODER_STRIP_HEIGHT;
    let renderer = KnobRenderer::new(width, height);

    let icon_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/icons/audio-lines.svg");
    let icon = load_icon_to_png_bytes(icon_path);
    if icon.is_none() {
        eprintln!("warning: could not load {icon_path}, rendering without an icon");
    }

    let states = states();
    let backgrounds = backgrounds(width, height);

    let sheet_w = backgrounds.len() as u32 * (width + GUTTER) + GUTTER;
    let sheet_h = states.len() as u32 * (height + GUTTER) + GUTTER;
    let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, Rgba([24, 24, 27, 255]));

    for (row, (label, params)) in states.iter().enumerate() {
        let Some((rgba, w, h)) = renderer.render_internal_png(params, icon.clone()) else {
            eprintln!("render failed for state {label}");
            continue;
        };
        let strip = RgbaImage::from_raw(w, h, rgba).expect("renderer returned a well-formed buffer");

        for (col, (_, background)) in backgrounds.iter().enumerate() {
            let mut cell = background.clone();
            over(&mut cell, &strip);

            let x = GUTTER + col as u32 * (width + GUTTER);
            let y = GUTTER + row as u32 * (height + GUTTER);
            image::imageops::overlay(&mut sheet, &cell, x as i64, y as i64);
        }
    }

    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/preview");
    std::fs::create_dir_all(dir).expect("create target/preview");
    let out = format!("{dir}/strip.png");
    sheet.save(&out).expect("write contact sheet");

    println!("wrote {out}");
    println!(
        "  columns: {}",
        backgrounds
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  rows:    {}",
        states
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    );

    render_slider_sheet(dir);
}

/// Slider keys share the strip's visual language, so they get the same treatment: each state
/// over each background. A slider spans two keys, so every cell shows the top and bottom halves
/// stacked — they must line up into one continuous fader.
fn render_slider_sheet(dir: &str) {
    let size = KEYPAD_SIZE;
    let renderer = SliderRenderer::new(size);
    let backgrounds = backgrounds(size, size * 2);
    let states = slider_states();

    let sheet_w = backgrounds.len() as u32 * (size + GUTTER) + GUTTER;
    let sheet_h = states.len() as u32 * (size * 2 + GUTTER) + GUTTER;
    let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, Rgba([24, 24, 27, 255]));

    for (row, (label, params)) in states.iter().enumerate() {
        for (col, (_, background)) in backgrounds.iter().enumerate() {
            let mut cell = background.clone();
            for (half, is_top) in [(0u32, true), (1, false)] {
                let Some((rgba, w, h)) = renderer.render_internal_png(params, is_top, false) else {
                    eprintln!("slider render failed for {label}");
                    continue;
                };
                let key = RgbaImage::from_raw(w, h, rgba).expect("well-formed buffer");
                over_at(&mut cell, &key, 0, half * size);
            }
            let x = GUTTER + col as u32 * (size + GUTTER);
            let y = GUTTER + row as u32 * (size * 2 + GUTTER);
            image::imageops::overlay(&mut sheet, &cell, x as i64, y as i64);
        }
    }

    let out = format!("{dir}/slider.png");
    sheet.save(&out).expect("write slider sheet");
    println!("wrote {out}");
    println!(
        "  rows:    {}",
        states
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn slider_states() -> Vec<(&'static str, RenderParams)> {
    let base = RenderParams {
        meters_enabled: true,
        is_source: true,
        ..Default::default()
    };

    vec![
        ("v65/m40", RenderParams { volume: 65, meter_value: 40, ..base.clone() }),
        ("v100/m88", RenderParams { volume: 100, meter_value: 88, ..base.clone() }),
        // Same two edge cases the strip has: a fill shorter than the bar is wide, and a level
        // running well past the fader.
        ("v5/m55", RenderParams { volume: 5, meter_value: 55, ..base.clone() }),
        ("v0/m0", RenderParams { volume: 0, meter_value: 0, ..base.clone() }),
        (
            "target/clipping",
            RenderParams { volume: 80, meter_value: 98, is_source: false, ..base },
        ),
    ]
}

/// Source-over composite of the rendered image onto an opaque background.
fn over(background: &mut RgbaImage, top: &RgbaImage) {
    over_at(background, top, 0, 0);
}

fn over_at(background: &mut RgbaImage, top: &RgbaImage, ox: u32, oy: u32) {
    for (x, y, src) in top.enumerate_pixels() {
        let a = src[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        let (dx, dy) = (x + ox, y + oy);
        if dx >= background.width() || dy >= background.height() {
            continue;
        }
        let dst = background.get_pixel_mut(dx, dy);
        for c in 0..3 {
            dst[c] = (src[c] as f32 * a + dst[c] as f32 * (1.0 - a)).round() as u8;
        }
    }
}

fn backgrounds(width: u32, height: u32) -> Vec<(&'static str, RgbaImage)> {
    let solid = |r, g, b| RgbaImage::from_pixel(width, height, Rgba([r, g, b, 255]));

    // A saturated gradient and a high-frequency checkerboard are the two cases a flat scrim
    // would have hidden and shadows have to handle.
    let mut gradient = RgbaImage::new(width, height);
    for (x, y, px) in gradient.enumerate_pixels_mut() {
        let u = x as f32 / width as f32;
        let v = y as f32 / height as f32;
        *px = Rgba([
            (40.0 + 200.0 * u) as u8,
            (90.0 + 120.0 * v) as u8,
            (200.0 - 120.0 * u) as u8,
            255,
        ]);
    }

    let mut checker = RgbaImage::new(width, height);
    for (x, y, px) in checker.enumerate_pixels_mut() {
        let on = ((x / 8) + (y / 8)) % 2 == 0;
        *px = if on {
            Rgba([235, 235, 235, 255])
        } else {
            Rgba([70, 70, 70, 255])
        };
    }

    vec![
        ("black", solid(0, 0, 0)),
        ("white", solid(255, 255, 255)),
        ("gradient", gradient),
        ("checker", checker),
    ]
}

fn states() -> Vec<(&'static str, RenderParams)> {
    let base = RenderParams {
        meters_enabled: true,
        show_volume: true,
        ..Default::default()
    };

    vec![
        (
            "source/mix-a",
            RenderParams {
                name: "Chat".into(),
                volume: 65,
                is_source: true,
                meter_value: 40,
                ..base.clone()
            },
        ),
        (
            "source/mix-b/linked/full",
            RenderParams {
                name: "Game".into(),
                volume: 100,
                is_source: true,
                meter_value: 88,
                mix_b_active: true,
                source_volumes_linked: true,
                ..base.clone()
            },
        ),
        (
            "target/long-name",
            RenderParams {
                name: "Headphones (USB Audio)".into(),
                volume: 45,
                is_source: false,
                meter_value: 0,
                ..base.clone()
            },
        ),
        (
            "muted/profile-2",
            RenderParams {
                name: "Music".into(),
                volume: 70,
                is_source: true,
                meter_value: 20,
                mute_profile: 1,
                mute_profile_muted: true,
                ..base.clone()
            },
        ),
        (
            // The case the nested meter has to survive: the level runs well past the fader, so
            // the lane crosses from the accent fill onto the darker track.
            "low-volume/hot-signal",
            RenderParams {
                name: "Browser".into(),
                volume: 25,
                is_source: true,
                meter_value: 85,
                ..base.clone()
            },
        ),
        (
            // A fill narrower than the bar is tall: its corners must still follow the track's.
            "tiny-volume",
            RenderParams {
                name: "Browser".into(),
                volume: 5,
                is_source: true,
                meter_value: 55,
                device_color: Some((236, 72, 60)),
                ..base.clone()
            },
        ),
        (
            "zero-volume",
            RenderParams {
                name: "System".into(),
                volume: 0,
                is_source: true,
                meter_value: 0,
                ..base.clone()
            },
        ),
        (
            "device-colour/clipping",
            RenderParams {
                name: "Microphone".into(),
                volume: 95,
                is_source: true,
                meter_value: 98,
                device_color: Some((236, 72, 153)),
                ..base.clone()
            },
        ),
        (
            "meters-off",
            RenderParams {
                name: "Line In".into(),
                volume: 55,
                is_source: true,
                meters_enabled: false,
                ..base.clone()
            },
        ),
        (
            // Readout hidden: the name takes the reclaimed width back.
            "no-readout/long-name",
            RenderParams {
                name: "Headphones (USB Audio)".into(),
                volume: 55,
                is_source: true,
                meter_value: 35,
                show_volume: false,
                ..base
            },
        ),
    ]
}
