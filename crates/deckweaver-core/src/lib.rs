//! DeckWeaver core - PipeWeaver IPC, device state, and image rendering.

mod action;
mod config;
mod core;
mod devices;
mod dimensions;
mod icon_loader;
mod render;

pub use action::{ActionConfig, ActionState, ActionType, CachedBaseRender, CachedIcon};
pub use config::{mute_profile_muted, populate_common_fields};
pub use core::{DeckWeaverCore, PendingUpdate, DEFAULT_PORT, VERSION};
pub use devices::{Device, DeviceColor, DeviceType, HardwareDevice, Status};
pub use dimensions::{
    action_dimensions, ControllerKind, ENCODER_ICON_SIZE, ENCODER_STRIP_HEIGHT,
    ENCODER_STRIP_WIDTH, KEYPAD_SIZE,
};
pub use icon_loader::{load_icon_to_png_bytes, svg_data_to_png_bytes};
pub use render::{ButtonRenderer, KnobRenderer, RenderParams, SliderRenderer};
