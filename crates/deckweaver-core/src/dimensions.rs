use crate::action::ActionType;

/// Stream Deck MK.2 / Plus key resolution.
pub const KEYPAD_SIZE: u32 = 144;
/// Stream Deck+ encoder LCD strip.
pub const ENCODER_STRIP_WIDTH: u32 = 200;
pub const ENCODER_STRIP_HEIGHT: u32 = 100;
/// Square actions placed on encoder slots (non-knob).
pub const ENCODER_ICON_SIZE: u32 = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerKind {
    Keypad,
    Encoder,
}

/// Render/output dimensions for an action on a given controller surface.
pub fn action_dimensions(action_type: ActionType, controller: ControllerKind) -> (u32, u32) {
    match action_type {
        ActionType::Knob => (ENCODER_STRIP_WIDTH, ENCODER_STRIP_HEIGHT),
        _ if controller == ControllerKind::Encoder => (ENCODER_ICON_SIZE, ENCODER_ICON_SIZE),
        _ => (KEYPAD_SIZE, KEYPAD_SIZE),
    }
}
