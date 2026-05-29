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
DEFAULT_PORT = _core.DEFAULT_PORT
DeckWeaverCore = _core.DeckWeaverCore
ActionConfig = _core.ActionConfig
ActionType = _core.ActionType
Device = _core.Device
DeviceColor = _core.DeviceColor
DeviceType = _core.DeviceType
HardwareDevice = _core.HardwareDevice
KnobRenderer = _core.KnobRenderer
SliderRenderer = _core.SliderRenderer
ButtonRenderer = _core.ButtonRenderer
load_icon_to_png = _core.load_icon_to_png

__version__ = VERSION
__all__ = [
    "VERSION",
    "DEFAULT_PORT",
    "DeckWeaverCore",
    "ActionConfig",
    "ActionType",
    "Device",
    "DeviceColor",
    "DeviceType",
    "HardwareDevice",
    "KnobRenderer",
    "SliderRenderer",
    "ButtonRenderer",
    "load_icon_to_png",
]
