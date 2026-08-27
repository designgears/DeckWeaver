//! DeckWeaver core - PipeWeaver IPC, device state, and image rendering.

mod action;
mod config;
mod core;
mod devices;
mod dimensions;
mod focus;
mod icon_loader;
mod pulse;
mod render;

pub use action::{ActionConfig, ActionState, ActionType, CachedBaseRender, CachedIcon, IconSizing};
pub use config::{mute_profile_muted, populate_common_fields};
pub use core::{DeckWeaverCore, PendingUpdate, DEFAULT_PORT, VERSION};
pub use devices::{Device, DeviceColor, DeviceType, HardwareDevice, Status};
pub use dimensions::{
    action_dimensions, ControllerKind, ENCODER_ICON_SIZE, ENCODER_STRIP_HEIGHT,
    ENCODER_STRIP_WIDTH, KEYPAD_SIZE,
};
pub use icon_loader::{
    dominant_accent, find_desktop_id_for_app, find_desktop_name_for_app, find_icon_by_name, find_icon_for_app, load_icon_native_png_bytes, load_icon_to_png_bytes,
    svg_data_to_png_bytes,
};
pub use focus::{detect_backend, is_same_or_descendant, Backend, FocusTracker, FocusedWindow};
pub use pulse::{
    app_key_from_device_id, is_focused_device_id, AppStream, PulseBackend, APP_DEVICE_PREFIX,
    FOCUSED_APP_KEY, FOCUSED_DEVICE_ID,
};
pub use render::{ButtonRenderer, KnobRenderer, RenderParams, SliderRenderer, SLIDER_ICON_ALPHA};
