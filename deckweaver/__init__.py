"""DeckWeaver - Stream Deck plugin for PipeWeaver audio control"""

try:
    from . import _core
except ImportError as exc:
    raise ImportError(
        f"deckweaver._core is not built. "
        f"Build with `./build.sh release` or `pip install .`. "
        f"Original error: {exc}"
    ) from exc

# Re-export public API
VERSION = _core.VERSION
FOCUSED_DEVICE_ID = _core.FOCUSED_DEVICE_ID
DeckWeaverCore = _core.DeckWeaverCore
ActionConfig = _core.ActionConfig
ActionType = _core.ActionType
AppStream = _core.AppStream
Device = _core.Device
DeviceColor = _core.DeviceColor
DeviceType = _core.DeviceType
HardwareDevice = _core.HardwareDevice
action_dimensions = _core.action_dimensions

__version__ = VERSION
__all__ = [
    "VERSION",
    "FOCUSED_DEVICE_ID",
    "DeckWeaverCore",
    "ActionConfig",
    "ActionType",
    "AppStream",
    "Device",
    "DeviceColor",
    "DeviceType",
    "HardwareDevice",
    "action_dimensions",
]
