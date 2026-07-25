//! Visual tokens for the Stream Deck+ encoder strip.
//!
//! The strip renders onto a fully transparent pixmap so the background the user configured in
//! OpenDeck / StreamController shows through. Nothing here may paint full-bleed; legibility on
//! an arbitrary background comes from the shadow/halo tokens and the hairline bar edges.

use super::common::{OutlineSpec, Rgba};

// ---------------------------------------------------------------------------
// Colour tokens
// ---------------------------------------------------------------------------

pub const TEXT_PRIMARY: Rgba = Rgba::rgb(255, 255, 255);
pub const TEXT_SECONDARY: Rgba = Rgba::new(226, 232, 236, 235);
/// Primary text once the channel is muted.
pub const TEXT_DIMMED: Rgba = Rgba::new(255, 255, 255, 160);

/// Backing for the status chips. Opaque enough that chip text needs no outline of its own, and
/// it is what keeps the small 12px labels readable over a light background.
pub const CHIP_BG: Rgba = Rgba::new(0, 0, 0, 180);

/// Surround for text and the device icon. Heavy on purpose: white content on a white
/// background has nothing else holding it up now that the panel is gone.
pub const CONTENT_OUTLINE: OutlineSpec = OutlineSpec {
    halo: Rgba::new(0, 0, 0, 115),
    drop: Rgba::new(0, 0, 0, 120),
    drop_offset: (0, 3),
};

/// Deliberately a mid-dark grey rather than translucent black: the track has to show how much
/// range is left, and pure black would vanish on a black background.
pub const BAR_TRACK: Rgba = Rgba::new(64, 68, 75, 215);
/// Hairline around the bars and chips, for definition against a light background.
pub const BAR_EDGE: Rgba = Rgba::new(0, 0, 0, 165);

/// Ring hugging the meter lane. The meter crosses from the accent fill onto the darker track,
/// so it needs a backdrop of its own to stay legible on both — but only where the meter
/// actually is. A full-width groove cut a visible channel through the parts of the fill the
/// meter never reaches.
pub const METER_EDGE: Rgba = Rgba::new(0, 0, 0, 145);
pub const METER_EDGE_WIDTH: f32 = 1.0;
/// Meter fill when the user configured no explicit colour.
pub const METER_DEFAULT: Rgba = Rgba::rgb(244, 248, 252);

pub const MUTE: Rgba = Rgba::rgb(229, 72, 77);
pub const MIX_A: Rgba = Rgba::rgb(87, 194, 206);
pub const MIX_B: Rgba = Rgba::rgb(240, 146, 44);
/// Tint blended into the meter as it approaches full scale.
pub const CLIP: Rgba = Rgba::rgb(255, 92, 87);
pub const CLIP_THRESHOLD: u8 = 90;

/// Alpha applied to the volume fill while the channel is muted.
pub const MUTED_FILL_ALPHA: u8 = 105;

// ---------------------------------------------------------------------------
// Layout — tuned for the 200x100 encoder zone
// ---------------------------------------------------------------------------

pub const PAD: f32 = 8.0;

pub const ICON_SIZE: f32 = 36.0;
pub const ICON_Y: f32 = 8.0;

pub const NAME_X: f32 = 52.0;
/// Vertically centred against the icon box.
pub const NAME_Y: f32 = ICON_Y + (ICON_SIZE - NAME_H) * 0.5;
pub const NAME_H: f32 = 18.0;
pub const NAME_SIZE: f32 = 16.0;
/// Gap between the device name and the volume readout beside it.
pub const NAME_GAP: f32 = 6.0;

/// Optional volume percentage, top right, sharing the name's baseline. Off, the name simply
/// takes the space back.
pub const VOLUME_SLOT_W: f32 = 46.0;
pub const VOLUME_SIZE: f32 = 16.0;

pub const LABEL_SIZE: f32 = 12.0;

// Status chips, all three along the bottom: mute left, mix centred, link right.
pub const CHIP_H: f32 = 18.0;
pub const CHIP_RADIUS: f32 = CHIP_H * 0.5;
pub const CHIP_EDGE_WIDTH: f32 = 1.0;
pub const MIX_CHIP_W: f32 = 46.0;
pub const MUTE_CHIP_W: f32 = 54.0;
pub const LINK_CHIP_W: f32 = 54.0;
pub const CHIP_Y_BOTTOM: f32 = 76.0;

pub const BAR_Y: f32 = 50.0;
pub const BAR_H: f32 = 18.0;
pub const BAR_EDGE_WIDTH: f32 = 1.5;

// Meter lane, recessed into the volume bar and centred in it.
pub const METER_H: f32 = 6.0;
/// Kept clear of the bar's rounded ends and its edge stroke.
pub const METER_INSET_X: f32 = 5.0;
