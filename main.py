"""StreamController plugin entry point - thin wrapper over Rust core"""

import json
import logging
import os
import time
from typing import Any, Optional

from PIL import Image
from loguru import logger as log

from src.backend.PluginManager.ActionBase import ActionBase
from src.backend.PluginManager.ActionHolder import ActionHolder
from src.backend.PluginManager.ActionInputSupport import ActionInputSupport
from src.backend.PluginManager.PluginBase import PluginBase
from src.backend.DeckManagement.InputIdentifier import Input
import globals as gl

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import Adw, GdkPixbuf, GLib, Gtk

# Configure logging for Rust before importing the module
# This routes Rust's tracing::info!, warn!, error! to Python's logging
logging.getLogger("deckweaver").setLevel(logging.DEBUG)

from .deckweaver import (
    FOCUSED_DEVICE_ID,
    VERSION,
    DeckWeaverCore,
    ActionConfig,
    ActionType,
    Device,
    DeviceType,
    action_dimensions,
)

PLUGIN_DIR = os.path.dirname(os.path.abspath(__file__))
DEFAULT_BUTTON_ICON_PATH = os.path.join(
    PLUGIN_DIR, "assets", "icons", "audio-lines.svg"
)
LANGUAGE_CODES = ("auto", "en_US", "es_ES", "zh_CN", "fr_FR", "de_DE")

# Press-and-hold on the volume keys: one step on press, then a step every REPEAT_INTERVAL_MS once
# the key has been held for HOLD_DELAY_MS. MAX_HOLD_REPEATS only matters if a release is lost.
HOLD_DELAY_MS = 400
# Time between steps while held; each key can change it with its "hold repeat speed" setting.
DEFAULT_REPEAT_INTERVAL_MS = 400
MIN_REPEAT_INTERVAL_MS = 50
MAX_REPEAT_INTERVAL_MS = 1000
MAX_HOLD_REPEATS = 100

# Single shared core instance
_core: Optional[DeckWeaverCore] = None
_poll_timeout_id: Optional[int] = None
_action_callbacks: dict[str, tuple[Any, Any]] = {}  # action_id -> (image_callback, label_callback)


def _poll_updates() -> bool:
    """Poll for pending updates from Rust core and process them"""
    if _core is None:
        return True  # Continue polling
    
    try:
        updates = _core.get_pending_updates()
        for action_id, update_dict in updates.items():
            image_cb, label_cb = _action_callbacks.get(action_id, (None, None))
            
            # Process image update
            if image_cb is not None and "image" in update_dict:
                image_data = update_dict["image"]
                if image_data is not None:
                    width = update_dict.get("width")
                    height = update_dict.get("height")
                    image_cb(image_data, width, height)
            
            # Process label update
            if label_cb is not None and "label" in update_dict:
                label_data = update_dict["label"]
                if label_data is not None:
                    label_cb(label_data)
    except Exception as e:
        log.error(f"Error polling updates: {e}")
    
    return True  # Continue polling


def _get_core() -> DeckWeaverCore:
    """Get or create the shared core instance"""
    global _core, _poll_timeout_id
    if _core is None:
        _core = DeckWeaverCore()
        _core.start()
        # Start polling timer (~30fps = ~33ms interval, matches render rate)
        if _poll_timeout_id is None:
            _poll_timeout_id = GLib.timeout_add(33, _poll_updates)
    return _core


def _release_core():
    """Stop and release the core"""
    global _core, _poll_timeout_id
    if _poll_timeout_id is not None:
        GLib.source_remove(_poll_timeout_id)
        _poll_timeout_id = None
    if _core is not None:
        _core.stop()
        _core = None
    _action_callbacks.clear()


class DeckWeaver(PluginBase):
    """Main plugin class for StreamController"""

    def __init__(self):
        super().__init__()
        self.lm = self.locale_manager
        self._core = _get_core()
        self._load_settings()
        self._register_actions()

    def _load_settings(self):
        settings = self.get_settings()
        language = settings.get("language", "auto")
        if language != "auto":
            self.lm.set_language(language)
        else:
            self.lm.set_to_os_default()

    def _register_actions(self):
        self.register(
            plugin_name=self.lm.get("plugin.name"),
            github_repo="https://github.com/designgears/DeckWeaver",
            plugin_version=VERSION,
            app_version="1.5.0-beta",
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=KnobAction,
                action_id_suffix="Knob",
                action_name=self.lm.get("actions.knob.name"),
                action_support={
                    Input.Key: ActionInputSupport.UNSUPPORTED,
                    Input.Dial: ActionInputSupport.SUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=ButtonAction,
                action_id_suffix="Button",
                action_name=self.lm.get("actions.button.name", "PipeWeaver Volume"),
                action_support={
                    Input.Key: ActionInputSupport.SUPPORTED,
                    Input.Dial: ActionInputSupport.UNSUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=SourceSwitchButtonAction,
                action_id_suffix="SourceSwitchButton",
                action_name=self.lm.get(
                    "actions.source_switch_button.name",
                    "PipeWeaver Output Device Switch",
                ),
                action_support={
                    Input.Key: ActionInputSupport.SUPPORTED,
                    Input.Dial: ActionInputSupport.UNSUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=PhysicalSourceSwitchButtonAction,
                action_id_suffix="PhysicalSourceSwitchButton",
                action_name=self.lm.get(
                    "actions.physical_source_switch_button.name",
                    "PipeWeaver Input Device Switch",
                ),
                action_support={
                    Input.Key: ActionInputSupport.SUPPORTED,
                    Input.Dial: ActionInputSupport.UNSUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=RouteToggleButtonAction,
                action_id_suffix="RouteToggleButton",
                action_name=self.lm.get(
                    "actions.route_toggle_button.name",
                    "PipeWeaver Route Toggle",
                ),
                action_support={
                    Input.Key: ActionInputSupport.SUPPORTED,
                    Input.Dial: ActionInputSupport.UNSUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=SliderAction,
                action_id_suffix="Slider",
                action_name=self.lm.get("actions.slider.name", "PipeWeaver Slider"),
                action_support={
                    Input.Key: ActionInputSupport.SUPPORTED,
                    Input.Dial: ActionInputSupport.UNSUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=AppKnobAction,
                action_id_suffix="AppKnob",
                action_name=self.lm.get("actions.app_knob.name", "App Volume Knob"),
                action_support={
                    Input.Key: ActionInputSupport.UNSUPPORTED,
                    Input.Dial: ActionInputSupport.SUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=AppButtonAction,
                action_id_suffix="AppButton",
                action_name=self.lm.get("actions.app_button.name", "App Volume"),
                action_support={
                    Input.Key: ActionInputSupport.SUPPORTED,
                    Input.Dial: ActionInputSupport.UNSUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

        self.add_action_holder(
            ActionHolder(
                plugin_base=self,
                action_base=AppSliderAction,
                action_id_suffix="AppSlider",
                action_name=self.lm.get("actions.app_slider.name", "App Volume Slider"),
                action_support={
                    Input.Key: ActionInputSupport.SUPPORTED,
                    Input.Dial: ActionInputSupport.UNSUPPORTED,
                    Input.Touchscreen: ActionInputSupport.SUPPORTED,
                },
            )
        )

    def get_settings_area(self) -> Adw.PreferencesGroup:
        languages = [
            (code, self.lm.get(f"settings.language.name.{code}"))
            for code in LANGUAGE_CODES
        ]

        self.language_model = Gtk.StringList.new([name for _, name in languages])
        self.language_dropdown = Adw.ComboRow(
            model=self.language_model,
            title=self.lm.get("settings.language.label"),
        )

        current = self.get_settings().get("language", "auto")
        for i, (code, _) in enumerate(languages):
            if code == current:
                self.language_dropdown.set_selected(i)
                break

        self.language_dropdown.connect("notify::selected", self._on_language_changed)

        version_row = Adw.ActionRow(
            title=self.lm.get("settings.version.label"), subtitle=VERSION
        )
        version_row.set_activatable(False)

        group = Adw.PreferencesGroup()
        group.add(self.language_dropdown)
        group.add(version_row)
        return group

    def _on_language_changed(self, combo: Adw.ComboRow, _):
        idx = combo.get_selected()
        if idx < len(LANGUAGE_CODES):
            code = LANGUAGE_CODES[idx]
            settings = self.get_settings()
            settings["language"] = code
            self.set_settings(settings)
            self._load_settings()

    def on_disable(self):
        _release_core()


class BaseAction(ActionBase):
    """Base action with shared Rust core functionality"""

    ACTION_TYPE = ActionType.knob()  # Override in subclasses
    MIN_VOLUME_STEP = 1
    MAX_VOLUME_STEP = 20
    DEFAULT_VOLUME_STEP = 5
    COLOR_METER = (0, 0, 0, 255)
    VOLUME_STEP_RANGE = (MIN_VOLUME_STEP, MAX_VOLUME_STEP)
    VOLUME_STEP_SUBTITLE: Optional[str] = None
    SHOW_METERS_ROW = False
    SHOW_HOLD_REPEAT_ROW = False
    SHOW_VOLUME_ROW = False
    # A stored device id with no stored type is resolved by the core. When the id matches both a
    # source and a target, this picks the target (the output device switch, whose device always is).
    PREFER_TARGET_DEVICE = False

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.has_configuration = True
        self._core = _get_core()
        self._action_id = f"{id(self)}"  # Unique ID for this action instance
        self._device_id: Optional[str] = None
        self._device_name: Optional[str] = None
        self._volume_step = self.DEFAULT_VOLUME_STEP
        self._meters_enabled = True
        self._show_volume = True
        self._meter_invert_color = True
        self._meter_color: Optional[tuple[int, int, int, int]] = None
        self._volume_bar_color: Optional[tuple[int, int, int, int]] = None
        self._icon_path: Optional[str] = None
        self._device_type: Optional[DeviceType] = None
        self._source_mix = "A"
        # Read by _build_config for every knob-type action. Only KnobAction loads real values;
        # the app knob has no mute profiles, and without these defaults its config build raised
        # AttributeError, so the App Volume Knob never registered with the core.
        self._mute_profile_index = 0
        self._mute_profile_data: list[bool] = [False, False]
        self.device_expander: Optional[Adw.ExpanderRow] = None
        self.icon_row: Optional[Adw.ActionRow] = None
        self.icon_preview: Optional[Gtk.Image] = None
        self.output_expander: Optional[Adw.ExpanderRow] = None
        self.source_expander: Optional[Adw.ExpanderRow] = None
        self._hold_timeout_id: Optional[int] = None
        self._hold_repeat_ms = DEFAULT_REPEAT_INTERVAL_MS

    # Press-and-hold. Key events may arrive off the GTK main loop, so every change to the hold
    # timer is queued onto it with GLib.idle_add: the press and its release are then applied in
    # the order they happened, and the timer id is only ever touched from one thread. Only live
    # GLib sources are ever stored in _hold_timeout_id, so _hold_stop can always remove it.

    def _hold_start(self, step_fn):
        """Step now, then keep stepping while the key is held, until `_hold_stop`."""
        GLib.idle_add(self._hold_start_main, step_fn)

    def _hold_stop(self):
        GLib.idle_add(self._hold_stop_main)

    def _hold_stop_main(self):
        if self._hold_timeout_id is not None:
            GLib.source_remove(self._hold_timeout_id)
            self._hold_timeout_id = None
        return False

    def _hold_start_main(self, step_fn):
        self._hold_stop_main()
        remaining = [MAX_HOLD_REPEATS]

        def step() -> bool:
            try:
                step_fn()
                return True
            except Exception as e:  # never let a failed step leave a timer running
                log.error(f"DeckWeaver: hold step failed: {e}")
                return False

        def tick():
            remaining[0] -= 1
            if remaining[0] >= 0 and step():
                return True
            self._hold_timeout_id = None  # this source ends now
            return False

        def begin():
            # the delay source ends here either way; the repeat source (if any) replaces it
            if tick():
                self._hold_timeout_id = GLib.timeout_add(self._hold_repeat_ms, tick)
            return False

        if step():
            self._hold_timeout_id = GLib.timeout_add(HOLD_DELAY_MS, begin)
        return False

    def _handle_hold_key(self, event: Any, step_fn):
        """Drive a volume key's press-and-hold from its key events: step on press, repeat while
        held, stop on release. The SHORT_UP after a quick tap is ignored (the press stepped)."""
        if event == Input.Key.Events.DOWN:
            self._hold_start(step_fn)
        elif event in (Input.Key.Events.UP, getattr(Input.Key.Events, "HOLD_STOP", None)):
            self._hold_stop()

    def _volume_key_event(self, event: Any, step: int, nudge, toggle_mute, extra: tuple = ()):
        """A volume key: `nudge(device_id, step, *extra)` on press and while held. A step of 0 is
        a mute key instead, `toggle_mute(device_id, *extra)` once per tap with no repeat."""
        if not self._device_id:
            self._hold_stop()
            return
        device_id = self._device_id
        if step == 0:
            if event == Input.Key.Events.SHORT_UP:
                toggle_mute(device_id, *extra)
            return
        # Bind the target now so a held key keeps stepping the device it started on.
        self._handle_hold_key(event, lambda: nudge(device_id, step, *extra))

    @staticmethod
    def _normalize_hold_repeat_ms(value: Any) -> int:
        try:
            ms = int(value)
        except (TypeError, ValueError):
            return DEFAULT_REPEAT_INTERVAL_MS
        return max(MIN_REPEAT_INTERVAL_MS, min(MAX_REPEAT_INTERVAL_MS, ms))

    def _on_hold_repeat_changed(self, spin_row: Adw.SpinRow, _):
        self._hold_repeat_ms = self._normalize_hold_repeat_ms(spin_row.get_value())
        self._persist_settings(hold_repeat_ms=self._hold_repeat_ms)

    def _slider_step(self) -> int:
        """A slider half's step; its sign picks the half, so 0 falls back to the default."""
        return self._volume_step or self.DEFAULT_VOLUME_STEP

    def _persist_settings(self, **updates):
        settings = self.get_settings()
        for key, value in updates.items():
            if value is None:
                settings.pop(key, None)
            else:
                settings[key] = value
        self.set_settings(settings)
        return settings

    def _uses_signed_volume_step(self) -> bool:
        return self.VOLUME_STEP_RANGE[0] < 0

    def _normalize_volume_step(self, value: int) -> int:
        if not self._uses_signed_volume_step():
            value = abs(value)
        min_step, max_step = self.VOLUME_STEP_RANGE
        return max(min_step, min(max_step, value))

    def _get_button_size(self) -> int:
        try:
            inp = self.get_input()
            fmt = inp.deck_controller.deck.key_image_format()
            return fmt["size"][0]
        except Exception:
            return 72

    def _get_knob_size(self) -> tuple[int, int]:
        try:
            inp = self.get_input()
            size = inp.get_image_size()
            if isinstance(size, (list, tuple)) and len(size) >= 2:
                width = int(size[0])
                height = int(size[1])
                if width > 0 and height > 0:
                    return width, height
        except Exception:
            pass
        return action_dimensions(ActionType.knob(), is_encoder=True)

    def _get_dimensions(self) -> tuple[int, int]:
        """Get width, height for this action type"""
        if self.ACTION_TYPE == ActionType.knob():
            return self._get_knob_size()
        else:
            size = self._get_button_size()
            return size, size

    def _load_color_tuple(self, settings: dict, key: str, default: Optional[tuple[int, int, int, int]] = None) -> Optional[tuple[int, int, int, int]]:
        """Load color tuple from settings, validating format"""
        color = settings.get(key)
        if color and isinstance(color, (list, tuple)) and len(color) >= 3:
            if len(color) == 3:
                return (int(color[0]), int(color[1]), int(color[2]), 255)
            return (int(color[0]), int(color[1]), int(color[2]), int(color[3]))
        return default

    def _resolved_icon_path(self) -> Optional[str]:
        if self._icon_path and os.path.exists(self._icon_path):
            return self._icon_path
        return None

    def _build_config(self) -> ActionConfig:
        """Build ActionConfig from current settings"""
        w, h = self._get_dimensions()
        config = ActionConfig(self._action_id, self.ACTION_TYPE, w, h)
        config.device_id = self._device_id
        config.device_type = self._device_type
        config.volume_step = self._volume_step
        config.meters_enabled = self._meters_enabled
        config.meter_invert = self._meter_invert_color
        
        if self._volume_bar_color:
            config.volume_bar_color = self._volume_bar_color
        if self._meter_color:
            config.meter_color = self._meter_color
        if self.ACTION_TYPE == ActionType.knob():
            config.apply_knob_settings(
                self._source_mix == "B",
                self._mute_profile_index,
                self._mute_profile_data,
                self._show_volume,
            )
        config.icon_path = self._resolved_icon_path()

        return config

    def _load_settings(self):
        settings = self.get_settings()
        self._device_id = settings.get("device_id")
        self._device_type = self._parse_device_type(settings.get("device_type"))
        volume_step = int(settings.get("volume_step", self.DEFAULT_VOLUME_STEP))
        self._volume_step = self._normalize_volume_step(volume_step)
        self._hold_repeat_ms = self._normalize_hold_repeat_ms(
            settings.get("hold_repeat_ms", DEFAULT_REPEAT_INTERVAL_MS)
        )
        self._meters_enabled = settings.get("meters_enabled", True)
        self._show_volume = settings.get("show_volume", True)
        self._meter_invert_color = settings.get("meter_invert_color", True)
        self._meter_color = self._load_color_tuple(settings, "meter_color", self.COLOR_METER)
        self._volume_bar_color = self._load_color_tuple(settings, "volume_bar_color")
        self._icon_path = settings.get("icon_path_from_picker")
        self._source_mix = "B" if settings.get("source_mix") == "B" else "A"
        if self._device_id and self._device_type is None:
            inferred_type = self._infer_device_type(self._device_id)
            if inferred_type is not None:
                self._device_type = inferred_type
                device_type = self._device_type_setting_value(inferred_type)
                if device_type is not None:
                    settings["device_type"] = device_type
                    self.set_settings(settings)
        
        # Try to get device name from core if we have device_id
        if self._device_id and not self._device_name:
            self._device_name = self._core.get_action_device_name(self._action_id)
            if not self._device_name:
                # Fallback: look through devices list
                device = self._get_selected_device()
                if device:
                    self._device_name = device.name

    def _register_with_core(self):
        """Register this action with the Rust core"""
        self._load_settings()
        config = self._build_config()
        self._core.register_action(config)
        # Store callbacks locally for polling mechanism
        _action_callbacks[self._action_id] = (self._on_image_update, self._on_label_update)

    def _unregister_from_core(self):
        """Unregister this action from the Rust core"""
        _action_callbacks.pop(self._action_id, None)
        self._core.unregister_action(self._action_id)

    def _update_config(self):
        """Update the Rust core with new config"""
        config = self._build_config()
        self._core.update_action(self._action_id, config)

    def _on_image_update(self, rgba_bytes: bytes, width: Optional[int], height: Optional[int]):
        """Called when a new image is ready for this action (from polling)"""
        def update():
            try:
                if width is None or height is None:
                    return
                image = Image.frombytes("RGBA", (width, height), rgba_bytes, "raw", "RGBA", 0, 1)
                self.set_media(image=image, update=True)
            except Exception as e:
                log.error(f"Error setting image: {e}")

        GLib.idle_add(update)

    def _on_label_update(self, label: str):
        """Called when the device name changes (from polling)"""
        def update():
            self._set_label(label[:25] if label else "")
        GLib.idle_add(update)

    def _set_label(self, text: str):
        """Set the label below the dial (top label for Stream Deck+)"""
        self.set_top_label(text, font_size=14)

    def _get_selected_source_mix_is_b(self) -> bool:
        return self._source_mix == "B"

    def _parse_device_type(self, value: Any) -> Optional[DeviceType]:
        if value == "source":
            return DeviceType.source()
        if value == "target":
            return DeviceType.target()
        return None

    def _device_type_setting_value(self, device_type: Optional[DeviceType]) -> Optional[str]:
        if device_type is None:
            return None
        if device_type.is_source():
            return "source"
        if device_type.is_target():
            return "target"
        return None

    def _infer_device_type(self, device_id: str) -> Optional[DeviceType]:
        return self._core.infer_device_type(device_id, self.PREFER_TARGET_DEVICE)

    def _set_selected_source_mix(self, mix: str):
        mix = "B" if mix == "B" else "A"
        self._source_mix = mix
        self._persist_settings(source_mix=mix)

    def _set_selected_device(self, device: Device):
        self._device_id = device.id
        self._device_name = device.name
        self._device_type = device.device_type
        self._persist_settings(
            device_id=device.id,
            device_name=device.name,
            device_type=self._device_type_setting_value(device.device_type),
        )

    def _get_selected_device(self) -> Optional[Device]:
        if not self._device_id:
            return None
        if self._device_type is None:
            self._device_type = self._infer_device_type(self._device_id)
        return self._core.get_device(self._device_id, self._device_type)

    def _is_selected_source_device(self) -> bool:
        if self._device_type is not None:
            return self._device_type.is_source()
        device = self._get_selected_device()
        return bool(device and device.device_type.is_source())

    def _toggle_source_link(self):
        if not self._device_id:
            return
        self._core.toggle_source_volumes_linked(self._device_id)

    def _clear_box_children(self, container: Gtk.Box):
        child = container.get_first_child()
        while child:
            next_child = child.get_next_sibling()
            container.remove(child)
            child = next_child

    def _icon_row_default_subtitle(self) -> str:
        return "Select an icon from StreamController's icon packs"

    def _build_icon_row(self) -> Adw.ActionRow:
        lm = self.plugin_base.lm
        icon_row = Adw.ActionRow()
        icon_row.set_title(lm.get("ui.custom_icon.title"))

        icon_content_box = Gtk.Box(
            orientation=Gtk.Orientation.HORIZONTAL,
            spacing=12,
            valign=Gtk.Align.CENTER,
        )

        self.icon_preview = Gtk.Image()
        self.icon_preview.set_size_request(20, 20)
        self.icon_preview.set_pixel_size(20)

        self.icon_row = icon_row
        if not (
            self._icon_path
            and os.path.exists(self._icon_path)
            and self._show_icon_preview(self._icon_path)
        ):
            icon_row.set_subtitle(self._icon_row_default_subtitle())
            self.icon_preview.set_visible(False)

        icon_content_box.append(self.icon_preview)

        icon_button_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        icon_picker_button = Gtk.Button(icon_name="folder-symbolic", valign=Gtk.Align.CENTER)
        icon_picker_button.set_tooltip_text("Choose icon")
        icon_picker_button.add_css_class("suggested-action")
        icon_picker_button.connect("clicked", self._on_icon_picker_clicked)
        icon_button_box.append(icon_picker_button)

        remove_icon_button = Gtk.Button(icon_name="edit-clear-symbolic", valign=Gtk.Align.CENTER)
        remove_icon_button.set_tooltip_text("Remove icon")
        remove_icon_button.connect("clicked", self._on_remove_icon_clicked)
        icon_button_box.append(remove_icon_button)

        icon_content_box.append(icon_button_box)
        icon_row.add_suffix(icon_content_box)
        return icon_row

    def _show_icon_preview(self, icon_path: str) -> bool:
        """Show `icon_path` in the icon row; False if it could not be loaded."""
        try:
            pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(
                icon_path, width=20, height=20, preserve_aspect_ratio=True
            )
            self.icon_preview.set_from_pixbuf(pixbuf)
            self.icon_preview.set_visible(True)
            icon_name = os.path.splitext(os.path.basename(icon_path))[0]
            self.icon_row.set_subtitle(f"Selected: {icon_name}")
            return True
        except Exception:
            return False

    def _backend_available(self) -> bool:
        """Which service this action needs before its config UI is worth showing. The app actions
        override this: they talk to the sound server, not PipeWeaver."""
        return self._core.is_available()

    def _backend_unavailable_title(self) -> str:
        return self.plugin_base.lm.get("ui.error.not_running.title")

    def _backend_unavailable_subtitle(self) -> str:
        return self.plugin_base.lm.get("ui.error.not_running.subtitle")

    def _device_picker_title(self) -> str:
        return self.plugin_base.lm.get("ui.device.title")

    def _backend_error_row(self) -> Adw.ActionRow:
        error_row = Adw.ActionRow()
        error_row.set_title(self._backend_unavailable_title())
        error_row.set_subtitle(self._backend_unavailable_subtitle())
        error_row.add_css_class("warning")
        return error_row

    def _build_device_expander(
        self, title: str, subtitle: str, on_expanded, on_refresh, min_height: int = 220
    ) -> tuple[Adw.ExpanderRow, Gtk.Box]:
        """An expander listing devices in a scrolled box, with a refresh button. Returns the
        expander and the box to fill."""
        expander = Adw.ExpanderRow()
        expander.set_title(title)
        expander.set_subtitle(subtitle)

        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        container.set_margin_start(30)
        container.set_margin_end(30)
        container.set_margin_bottom(12)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_min_content_height(min_height)
        scrolled.set_child(container)
        expander.add_row(scrolled)
        expander.connect("notify::expanded", on_expanded)

        refresh = Gtk.Button(
            icon_name="view-refresh-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text=self.plugin_base.lm.get("ui.refresh_devices.button"),
        )
        refresh.connect("clicked", on_refresh)
        expander.add_suffix(refresh)
        return expander, container

    @staticmethod
    def _build_spin_row(
        lower: int, upper: int, step: int, value: int, title: str, subtitle: str, on_changed
    ) -> Adw.SpinRow:
        row = Adw.SpinRow.new_with_range(lower, upper, step)
        row.set_value(value)  # before connecting, so loading the value is not saved back
        row.set_title(title)
        row.set_subtitle(subtitle)
        row.connect("notify::value", on_changed)
        return row

    @staticmethod
    def _build_switch_row(title: str, subtitle: str, active: bool, on_changed) -> Adw.ActionRow:
        row = Adw.ActionRow()
        row.set_title(title)
        row.set_subtitle(subtitle)
        switch = Gtk.Switch(valign=Gtk.Align.CENTER)
        switch.set_active(active)
        switch.connect("notify::active", on_changed)
        row.add_suffix(switch)
        return row

    def get_config_rows(self):
        """Return configuration UI rows - matching original GitHub style"""
        lm = self.plugin_base.lm
        self._load_settings()
        self.device_expander = None
        self.icon_row = None
        self.icon_preview = None

        if not self._backend_available():
            return [self._backend_error_row()]

        self.device_expander, self.device_container = self._build_device_expander(
            self._device_picker_title(),
            self._device_name or "No device selected",
            self._on_device_expander_expanded,
            self._on_refresh_clicked,
            min_height=300,
        )
        self._populate_device_list()

        min_step, max_step = self.VOLUME_STEP_RANGE
        rows = [
            self.device_expander,
            self._build_icon_row(),
            self._build_spin_row(
                min_step,
                max_step,
                1,
                self._volume_step,
                lm.get("ui.volume_step.title"),
                self.VOLUME_STEP_SUBTITLE or lm.get("ui.volume_step.subtitle"),
                self._on_volume_step_changed,
            ),
        ]

        if self.SHOW_HOLD_REPEAT_ROW:
            rows.append(
                self._build_spin_row(
                    MIN_REPEAT_INTERVAL_MS,
                    MAX_REPEAT_INTERVAL_MS,
                    50,
                    self._hold_repeat_ms,
                    lm.get("ui.hold_repeat.title", "Hold Repeat Speed (ms)"),
                    lm.get(
                        "ui.hold_repeat.subtitle",
                        "Time between steps while the key is held. Lower is faster.",
                    ),
                    self._on_hold_repeat_changed,
                )
            )

        if self.SHOW_METERS_ROW:
            rows.append(
                self._build_switch_row(
                    "Meters Enabled",
                    "Show audio level meters",
                    self._meters_enabled,
                    self._on_meters_enabled_changed,
                )
            )

        # Volume percentage (encoder strip only)
        if self.SHOW_VOLUME_ROW:
            rows.append(
                self._build_switch_row(
                    lm.get("ui.show_volume.title"),
                    lm.get("ui.show_volume.subtitle"),
                    self._show_volume,
                    self._on_show_volume_changed,
                )
            )

        return rows

    def _populate_device_list(self, retry_count: int = 0):
        """Populate the device list in the expander"""
        self._clear_box_children(self.device_container)
        devices = self._core.get_devices()

        if not devices:
            # Retry a few times if devices not loaded yet
            if retry_count < 5:
                loading_row = Adw.ActionRow()
                loading_row.set_title("Loading devices...")
                loading_row.set_sensitive(False)
                self.device_container.append(loading_row)
                GLib.timeout_add(500, lambda: self._populate_device_list(retry_count + 1) or False)
                return
            
            no_devices_row = Adw.ActionRow()
            no_devices_row.set_title("No devices found")
            no_devices_row.set_subtitle("Check that PipeWeaver is running")
            no_devices_row.set_sensitive(False)
            self.device_container.append(no_devices_row)
            return
        
        # Update device name if we have a selected device
        if self._device_id and not self._device_name:
            for d in devices:
                if d.id == self._device_id:
                    self._device_name = d.name
                    if self.device_expander is not None:
                        self.device_expander.set_subtitle(d.name)
                    break
        
        # Get sources and targets directly from Rust
        sources = self._core.get_sources()
        targets = self._core.get_targets()
        
        for device_group in (sources, targets):
            if not device_group:
                continue
            group = Adw.PreferencesGroup()
            group.set_margin_top(12)
            group.set_margin_bottom(6)
            for device in device_group:
                group.add(
                    self._build_device_row(
                        device, self._on_device_row_activated, device.id == self._device_id
                    )
                )
            self.device_container.append(group)

    def _build_device_row(self, device, callback, selected: bool) -> Adw.ActionRow:
        """Create a styled device row"""
        row = Adw.ActionRow()
        row.set_title(device.name)
        
        device_type = "Source" if int(device.device_type) == 0 else "Target"
        hw_type = "Physical" if device.is_physical else "Virtual"
        row.set_subtitle(f"{hw_type} {device_type}")
        
        # Add color indicator if available
        if device.color:
            color_dot = Gtk.Label()
            hex_color = f"#{device.color.red:02x}{device.color.green:02x}{device.color.blue:02x}"
            color_dot.set_markup(f"<span foreground='{hex_color}'>●</span>")
            row.add_suffix(color_dot)
        
        row.device_data = device
        row.set_activatable(True)
        row.connect("activated", callback)
        if selected:
            row.add_css_class("selected")
        return row

    def _fill_device_container(
        self, container: Gtk.Box, devices: list, build_row, empty_title: str, empty_subtitle: str
    ):
        """Replace `container`'s rows with one `build_row(device)` per device, or a disabled
        placeholder row when there are none."""
        self._clear_box_children(container)
        if not devices:
            row = Adw.ActionRow()
            row.set_title(empty_title)
            row.set_subtitle(empty_subtitle)
            row.set_sensitive(False)
            container.append(row)
            return

        group = Adw.PreferencesGroup()
        group.set_margin_top(12)
        group.set_margin_bottom(6)
        for device in devices:
            group.add(build_row(device))
        container.append(group)

    def _on_device_row_activated(self, row: Adw.ActionRow):
        """Handle device row activation"""
        device = row.device_data
        if device:
            self._set_selected_device(device)
            
            if self.device_expander is not None:
                self.device_expander.set_subtitle(device.name)
            
            self._update_config()

    def _on_device_expander_expanded(self, expander: Adw.ExpanderRow, _):
        """Reload devices when expander is opened"""
        if expander.get_expanded():
            self._populate_device_list()

    def _on_refresh_clicked(self, button: Gtk.Button):
        """Refresh device list"""
        self._populate_device_list()

    def _on_icon_picker_clicked(self, button: Gtk.Button):
        """Open icon picker"""
        try:
            if gl.app is None:
                return
            gl.app.let_user_select_asset(
                default_path="",
                callback_func=self._on_icon_selected,
                callback_args=(),
                callback_kwargs={}
            )
        except Exception as e:
            log.error(f"Error opening icon picker: {e}")

    def _on_icon_selected(self, icon_path: str, *args, **kwargs):
        """Handle icon selection"""
        if not icon_path:
            return
        
        self._icon_path = icon_path
        self._persist_settings(icon_path_from_picker=icon_path)
        
        if self.icon_preview is not None and os.path.exists(icon_path):
            self._show_icon_preview(icon_path)
        
        self._update_config()

    def _on_remove_icon_clicked(self, button: Gtk.Button):
        """Remove custom icon"""
        self._icon_path = None
        self._persist_settings(icon_path_from_picker=None)
        
        if self.icon_preview is not None:
            self.icon_preview.set_visible(False)
        if self.icon_row is not None:
            self.icon_row.set_subtitle(self._icon_row_default_subtitle())
        
        self._update_config()

    def _on_volume_step_changed(self, spin_row: Adw.SpinRow, _):
        value = int(spin_row.get_value())
        self._volume_step = self._normalize_volume_step(value)
        self._persist_settings(volume_step=self._volume_step)
        self._update_config()

    def _on_meters_enabled_changed(self, switch: Gtk.Switch, _):
        self._meters_enabled = switch.get_active()
        self._persist_settings(meters_enabled=self._meters_enabled)
        self._update_config()

    def _on_show_volume_changed(self, switch: Gtk.Switch, _):
        self._show_volume = switch.get_active()
        self._persist_settings(show_volume=self._show_volume)
        self._update_config()

    def on_enable(self):
        self._register_with_core()

    def on_ready(self):
        self._register_with_core()

    def on_disable(self):
        self._hold_stop()
        self._unregister_from_core()


class KnobAction(BaseAction):
    """Knob/dial action for volume control"""

    ACTION_TYPE = ActionType.knob()
    DOUBLE_TAP_WINDOW_MS = 275
    SHOW_METERS_ROW = True
    SHOW_VOLUME_ROW = True
    MUTE_PROFILE_COUNT = 2

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._pending_touch_tap_count = 0
        self._pending_touch_timeout_id: Optional[int] = None
        self._last_touch_tap_ts = 0.0

    def _load_settings(self):
        super()._load_settings()
        settings = self.get_settings()
        self._mute_profile_index = int(settings.get("mute_profile_index", 0))
        raw = settings.get("mute_profile_data")
        self._mute_profile_data = []
        if raw and isinstance(raw, str):
            try:
                parsed: list = json.loads(raw)
                for entry in parsed:
                    if isinstance(entry, dict):
                        self._mute_profile_data.append(bool(entry.get("a", entry.get("muted", False))))
                    elif isinstance(entry, bool):
                        self._mute_profile_data.append(entry)
                    else:
                        self._mute_profile_data.append(False)
            except (json.JSONDecodeError, TypeError):
                pass
        while len(self._mute_profile_data) < self.MUTE_PROFILE_COUNT:
            self._mute_profile_data.append(False)
        if len(self._mute_profile_data) > self.MUTE_PROFILE_COUNT:
            self._mute_profile_data = self._mute_profile_data[:self.MUTE_PROFILE_COUNT]

    def _set_knob_volume_relative(self, delta: int):
        if not self._device_id:
            return
        if self._is_selected_source_device():
            self._core.set_source_volume_relative(
                self._device_id,
                self._get_selected_source_mix_is_b(),
                delta,
            )
        else:
            self._core.set_volume_relative(self._device_id, delta, self._device_type)

    def _set_knob_mix(self, mix_b: bool):
        if not self._device_id:
            return
        if self._is_selected_source_device():
            self._set_selected_source_mix("B" if mix_b else "A")
        else:
            self._core.set_target_mix(self._device_id, mix_b)

    def _toggle_knob_mix(self):
        if not self._device_id:
            return
        if self._is_selected_source_device():
            self._set_selected_source_mix(
                "A" if self._get_selected_source_mix_is_b() else "B"
            )
            return

        self._core.toggle_target_mix(self._device_id)

    def _cancel_pending_touch_tap(self):
        if self._pending_touch_timeout_id is not None:
            GLib.source_remove(self._pending_touch_timeout_id)
            self._pending_touch_timeout_id = None
        self._pending_touch_tap_count = 0
        self._last_touch_tap_ts = 0.0

    def _handle_touchscreen_single_tap(self):
        """Cycle to the next mute profile (P1 -> P2 -> P3 -> P4 -> P1 ...)"""
        # Only selects which profile the press addresses. Pushing the stored value onto the new
        # profile here would clobber a mute the user made in PipeWeaver's own UI.
        self._mute_profile_index = (self._mute_profile_index + 1) % self.MUTE_PROFILE_COUNT
        self._persist_settings(mute_profile_index=self._mute_profile_index)
        self._update_config()

    def _toggle_mute_profile(self):
        """Flip the active profile's mute against the live server state, then remember it."""
        muted = self._core.toggle_mute_profile(self._build_config())
        if muted is not None:
            self._save_mute_for_profile(muted)

    def _save_mute_for_profile(self, muted: bool):
        """Save a mute state to the active profile and persist."""
        idx = self._mute_profile_index
        if idx >= len(self._mute_profile_data):
            return
        self._mute_profile_data[idx] = muted
        self._persist_settings(mute_profile_data=json.dumps(self._mute_profile_data))

    def _handle_touchscreen_double_tap(self):
        """Toggle between Mix A and Mix B"""
        self._toggle_knob_mix()

    def _flush_pending_touch_tap(self) -> bool:
        tap_count = self._pending_touch_tap_count
        self._pending_touch_timeout_id = None
        self._pending_touch_tap_count = 0
        self._last_touch_tap_ts = 0.0

        if tap_count >= 2:
            self._handle_touchscreen_double_tap()
        elif tap_count == 1:
            self._handle_touchscreen_single_tap()

        self._update_config()
        return False

    def _handle_touchscreen_short_press(self, data: Any):
        now = time.monotonic()
        within_window = (
            self._pending_touch_timeout_id is not None
            and (now - self._last_touch_tap_ts) * 1000.0 <= self.DOUBLE_TAP_WINDOW_MS
        )

        if within_window:
            self._pending_touch_tap_count += 1
            if self._pending_touch_timeout_id is not None:
                GLib.source_remove(self._pending_touch_timeout_id)
                self._pending_touch_timeout_id = None
            self._flush_pending_touch_tap()
            return

        self._cancel_pending_touch_tap()
        self._pending_touch_tap_count = 1
        self._last_touch_tap_ts = now
        self._pending_touch_timeout_id = GLib.timeout_add(
            self.DOUBLE_TAP_WINDOW_MS,
            self._flush_pending_touch_tap,
        )

    def event_callback(self, event: Any, data: Any):
        if not self._device_id:
            return

        if event == Input.Dial.Events.TURN_CW:
            self._set_knob_volume_relative(self._volume_step)
        elif event == Input.Dial.Events.TURN_CCW:
            self._set_knob_volume_relative(-self._volume_step)
        elif event == Input.Dial.Events.SHORT_UP:
            # Toggle the active profile and apply to the selected mix.
            if self._mute_profile_index < len(self._mute_profile_data):
                self._toggle_mute_profile()
            self._update_config()
        elif event == Input.Dial.Events.SHORT_TOUCH_PRESS:
            self._handle_touchscreen_short_press(data)
        elif event == Input.Dial.Events.LONG_TOUCH_PRESS:
            self._cancel_pending_touch_tap()
            if self._is_selected_source_device():
                self._toggle_source_link()
                self._update_config()


class ButtonAction(BaseAction):
    """Volume button (positive step = up, negative step = down, zero = mute)"""

    SHOW_HOLD_REPEAT_ROW = True

    ACTION_TYPE = ActionType.button()
    VOLUME_STEP_RANGE = (-20, 20)
    VOLUME_STEP_SUBTITLE = "Positive = vol up, Negative = vol down, Zero = mute toggle"

    def _resolved_icon_path(self) -> Optional[str]:
        return super()._resolved_icon_path() or (
            DEFAULT_BUTTON_ICON_PATH if os.path.exists(DEFAULT_BUTTON_ICON_PATH) else None
        )

    def event_callback(self, event: Any, data: Any):
        self._volume_key_event(
            event,
            self._volume_step,
            self._core.set_volume_relative,
            self._core.toggle_mute,
            (self._device_type,),
        )


class _HardwareSwitchButtonAction(ButtonAction):
    """Button that moves a PipeWeaver physical device onto a selected hardware device.

    Subclasses pick the direction (output or input): which devices are listed, the locale keys
    and English fallbacks, and which core call does the switch.
    """

    # Locale key prefix and fallbacks for the PipeWeaver device picker...
    DEVICE_KEY = ""
    DEVICE_TITLE = ""
    DEVICE_NONE = ""
    DEVICE_EMPTY_HINT = ""
    # ...and for the hardware device picker.
    HARDWARE_KEY = ""
    HARDWARE_TITLE = ""
    HARDWARE_EMPTY_HINT = ""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._hardware_device_node_id: Optional[int] = None
        self._hardware_device_description: Optional[str] = None
        self._hardware_device_name: Optional[str] = None

    def _pipeweaver_devices(self) -> list:
        raise NotImplementedError

    def _hardware_devices(self) -> list:
        raise NotImplementedError

    def _switch_hardware_device(self, device_id: str, node_id: int):
        raise NotImplementedError

    def _load_settings(self):
        super()._load_settings()
        settings = self.get_settings()
        self._hardware_device_description = settings.get("hardware_device_description")
        self._hardware_device_name = settings.get("hardware_device_name")

        # Resolve node_id from the stored description (which is stable across reboots)
        if self._hardware_device_description is not None and self._hardware_device_node_id is None:
            for device in self._hardware_devices():
                if device.description == self._hardware_device_description:
                    self._hardware_device_node_id = device.node_id
                    # Back-fill name if missing
                    if not self._hardware_device_name and device.name:
                        self._hardware_device_name = device.name
                    break

    def _hardware_device_subtitle(self) -> str:
        return self._hardware_device_name or self.plugin_base.lm.get(
            f"{self.HARDWARE_KEY}.none", "No physical device selected"
        )

    def get_config_rows(self):
        lm = self.plugin_base.lm
        self._load_settings()
        self.output_expander = None
        self.source_expander = None

        if not self._backend_available():
            return [self._backend_error_row()]

        self.output_expander, self.output_device_container = self._build_device_expander(
            lm.get(f"{self.DEVICE_KEY}.title", self.DEVICE_TITLE),
            self._device_name or lm.get(f"{self.DEVICE_KEY}.none", self.DEVICE_NONE),
            self._on_output_expander_expanded,
            self._on_output_refresh_clicked,
        )
        self._populate_output_device_list()

        self.source_expander, self.source_device_container = self._build_device_expander(
            lm.get(f"{self.HARDWARE_KEY}.title", self.HARDWARE_TITLE),
            self._hardware_device_subtitle(),
            self._on_source_expander_expanded,
            self._on_source_refresh_clicked,
            min_height=260,
        )
        self._populate_source_device_list()

        return [self.output_expander, self.source_expander, self._build_icon_row()]

    def _build_config(self) -> ActionConfig:
        config = super()._build_config()
        config.button_overlay = False
        return config

    def _populate_output_device_list(self):
        self._fill_device_container(
            self.output_device_container,
            self._pipeweaver_devices(),
            lambda device: self._build_device_row(
                device, self._on_output_row_activated, device.id == self._device_id
            ),
            self.plugin_base.lm.get(f"{self.DEVICE_KEY}.empty", "No devices found"),
            self.DEVICE_EMPTY_HINT,
        )

    def _populate_source_device_list(self):
        self._fill_device_container(
            self.source_device_container,
            self._hardware_devices(),
            self._build_hardware_device_row,
            self.plugin_base.lm.get(f"{self.HARDWARE_KEY}.empty", "No physical devices found"),
            self.HARDWARE_EMPTY_HINT,
        )

    def _build_hardware_device_row(self, device) -> Adw.ActionRow:
        row = Adw.ActionRow()
        row.set_title(device.description or device.name or f"Node {device.node_id}")
        row.set_subtitle(device.name or "Physical device")
        row.device_data = device
        row.set_activatable(True)
        row.connect("activated", self._on_source_row_activated)
        if device.node_id is not None and device.node_id == self._hardware_device_node_id:
            row.add_css_class("selected")
        return row

    def _on_output_row_activated(self, row: Adw.ActionRow):
        device = row.device_data
        if not device:
            return

        self._set_selected_device(device)

        if self.output_expander is not None:
            self.output_expander.set_subtitle(device.name)

        self._update_config()

    def _on_source_row_activated(self, row: Adw.ActionRow):
        device = row.device_data
        if not device:
            return

        self._hardware_device_node_id = device.node_id
        self._hardware_device_description = device.description
        self._hardware_device_name = device.name or device.description or (
            str(device.node_id) if device.node_id is not None else None
        )
        self._persist_settings(
            hardware_device_description=self._hardware_device_description,
            hardware_device_node_id=self._hardware_device_node_id,
            hardware_device_name=self._hardware_device_name,
        )

        if self.source_expander is not None:
            self.source_expander.set_subtitle(self._hardware_device_subtitle())

    def _on_output_expander_expanded(self, expander: Adw.ExpanderRow, _):
        if expander.get_expanded():
            self._populate_output_device_list()

    def _on_source_expander_expanded(self, expander: Adw.ExpanderRow, _):
        if expander.get_expanded():
            self._populate_source_device_list()

    def _on_output_refresh_clicked(self, button: Gtk.Button):
        self._populate_output_device_list()

    def _on_source_refresh_clicked(self, button: Gtk.Button):
        self._populate_source_device_list()

    def event_callback(self, event: Any, data: Any):
        if event != Input.Key.Events.SHORT_UP:
            return
        if not self._device_id or self._hardware_device_node_id is None:
            return

        self._switch_hardware_device(self._device_id, self._hardware_device_node_id)


class SourceSwitchButtonAction(_HardwareSwitchButtonAction):
    """Button that switches a hardware output target to a selected physical device."""

    PREFER_TARGET_DEVICE = True
    DEVICE_KEY = "ui.output_device"
    DEVICE_TITLE = "Hardware Output Device"
    DEVICE_NONE = "No hardware output selected"
    DEVICE_EMPTY_HINT = "Check that PipeWeaver has detected your output device"
    HARDWARE_KEY = "ui.physical_device"
    HARDWARE_TITLE = "Physical Output Device"
    HARDWARE_EMPTY_HINT = "Check that PipeWeaver has detected your hardware outputs"

    def _pipeweaver_devices(self) -> list:
        return self._core.get_physical_targets()

    def _hardware_devices(self) -> list:
        return self._core.get_output_hardware_devices()

    def _switch_hardware_device(self, device_id: str, node_id: int):
        self._core.switch_output_hardware_device(device_id, node_id)


class PhysicalSourceSwitchButtonAction(_HardwareSwitchButtonAction):
    """Button that switches a hardware input source to a selected physical device."""

    DEVICE_KEY = "ui.input_device"
    DEVICE_TITLE = "Hardware Input Device"
    DEVICE_NONE = "No hardware input selected"
    DEVICE_EMPTY_HINT = "Check that PipeWeaver has detected your input device"
    HARDWARE_KEY = "ui.physical_input_device"
    HARDWARE_TITLE = "Physical Input Device"
    HARDWARE_EMPTY_HINT = "Check that PipeWeaver has detected your hardware inputs"

    def _pipeweaver_devices(self) -> list:
        return self._core.get_physical_sources()

    def _hardware_devices(self) -> list:
        return self._core.get_input_hardware_devices()

    def _switch_hardware_device(self, device_id: str, node_id: int):
        self._core.switch_input_hardware_device(device_id, node_id)


class RouteToggleButtonAction(ButtonAction):
    """Button that turns the route from one source into one target on and off.

    For example, a microphone into headphones, to hear yourself only when you want to.
    """

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._route_target_id: Optional[str] = None
        self._route_target_name: Optional[str] = None
        self.target_expander: Optional[Adw.ExpanderRow] = None

    def _load_settings(self):
        super()._load_settings()
        settings = self.get_settings()
        self._route_target_id = settings.get("route_target_id")
        self._route_target_name = settings.get("route_target_name")

    def _build_config(self) -> ActionConfig:
        config = super()._build_config()
        # Drawn as a toggle (no +/- symbol) that reads muted while the route is off.
        config.volume_step = 0
        config.route_target_id = self._route_target_id
        return config

    def get_config_rows(self):
        lm = self.plugin_base.lm
        self._load_settings()
        self.source_expander = None
        self.target_expander = None

        if not self._backend_available():
            return [self._backend_error_row()]

        self.source_expander, self.route_source_container = self._build_device_expander(
            lm.get("ui.route_source.title", "Source"),
            self._device_name or lm.get("ui.route_source.none", "No source selected"),
            lambda e, _: e.get_expanded() and self._populate_route_sources(),
            lambda _b: self._populate_route_sources(),
        )
        self._populate_route_sources()

        self.target_expander, self.route_target_container = self._build_device_expander(
            lm.get("ui.route_target.title", "Target"),
            self._route_target_name or lm.get("ui.route_target.none", "No target selected"),
            lambda e, _: e.get_expanded() and self._populate_route_targets(),
            lambda _b: self._populate_route_targets(),
        )
        self._populate_route_targets()

        return [self.source_expander, self.target_expander, self._build_icon_row()]

    def _populate_route_sources(self):
        self._fill_device_container(
            self.route_source_container,
            self._core.get_sources(),
            lambda device: self._build_device_row(
                device, self._on_route_source_activated, device.id == self._device_id
            ),
            self.plugin_base.lm.get("ui.route_source.empty", "No devices found"),
            "Check that PipeWeaver is running",
        )

    def _populate_route_targets(self):
        self._fill_device_container(
            self.route_target_container,
            self._core.get_targets(),
            lambda device: self._build_device_row(
                device, self._on_route_target_activated, device.id == self._route_target_id
            ),
            self.plugin_base.lm.get("ui.route_target.empty", "No targets found"),
            "Check that PipeWeaver is running",
        )

    def _on_route_source_activated(self, row: Adw.ActionRow):
        device = row.device_data
        if not device:
            return
        self._set_selected_device(device)
        if self.source_expander is not None:
            self.source_expander.set_subtitle(device.name)
        self._populate_route_sources()
        self._update_config()

    def _on_route_target_activated(self, row: Adw.ActionRow):
        device = row.device_data
        if not device:
            return
        self._route_target_id = device.id
        self._route_target_name = device.name
        self._persist_settings(route_target_id=device.id, route_target_name=device.name)
        if self.target_expander is not None:
            self.target_expander.set_subtitle(device.name)
        self._populate_route_targets()
        self._update_config()

    def event_callback(self, event: Any, data: Any):
        if event != Input.Key.Events.SHORT_UP:
            return
        if not self._device_id or not self._route_target_id:
            return
        self._core.toggle_route(self._device_id, self._route_target_id)


class SliderAction(BaseAction):
    """Slider button (top/bottom half of virtual slider)"""

    ACTION_TYPE = ActionType.slider()
    VOLUME_STEP_RANGE = (-20, 20)
    VOLUME_STEP_SUBTITLE = "Positive = top slider, Negative = bottom"
    SHOW_HOLD_REPEAT_ROW = True
    SHOW_METERS_ROW = True

    def _build_config(self) -> ActionConfig:
        config = super()._build_config()
        settings = self.get_settings()
        config.orientation = settings.get("orientation", "vertical")
        config.is_top = self._slider_step() > 0
        return config

    def get_config_rows(self):
        rows = super().get_config_rows()

        orientation_row = Adw.ActionRow()
        orientation_row.set_title("Orientation")
        orientation_row.set_subtitle("Slider direction")

        self.orientation_combo = Gtk.ComboBoxText()
        self.orientation_combo.append("vertical", "Vertical")
        self.orientation_combo.append("horizontal", "Horizontal")
        self.orientation_combo.set_active_id(
            self.get_settings().get("orientation", "vertical")
        )
        self.orientation_combo.connect("changed", self._on_orientation_changed)
        orientation_row.add_suffix(self.orientation_combo)
        rows.append(orientation_row)

        return rows

    def _on_orientation_changed(self, combo: Gtk.ComboBoxText):
        self._persist_settings(orientation=combo.get_active_id())
        self._update_config()

    def event_callback(self, event: Any, data: Any):
        self._volume_key_event(
            event,
            self._slider_step(),
            self._core.set_volume_relative,
            None,
            (self._device_type,),
        )


class AppActionMixin:
    """Shared behaviour for the per-application volume actions.

    These bypass PipeWeaver entirely and talk to the sound server, so they work on a machine that
    has never run it. An action stores an "app:<binary>" device id, which survives the app being
    closed and reopened — sink input indices do not.
    """

    def _backend_available(self) -> bool:
        return self._core.is_pulse_available()

    def _backend_unavailable_title(self) -> str:
        return "No sound server available"

    def _backend_unavailable_subtitle(self) -> str:
        return "PulseAudio or PipeWire is not reachable"

    def _device_picker_title(self) -> str:
        return "Application"

    def _populate_device_list(self, retry_count: int = 0):
        """Replace the PipeWeaver device list with the apps currently playing audio."""
        self._clear_box_children(self.device_container)
        apps = self._core.get_apps()

        # Nothing is playing yet is a normal transient state right after login, so retry a few
        # times before settling on the empty message.
        if not apps:
            if retry_count < 5:
                loading_row = Adw.ActionRow()
                loading_row.set_title("Looking for apps…")
                loading_row.set_sensitive(False)
                self.device_container.append(loading_row)
                GLib.timeout_add(
                    500, lambda: self._populate_device_list(retry_count + 1) or False
                )
                return

            empty_row = Adw.ActionRow()
            empty_row.set_title("No apps are playing audio")
            empty_row.set_subtitle("Start playback in an app, then refresh")
            empty_row.set_sensitive(False)
            self.device_container.append(empty_row)

        if self._device_id and not self._device_name:
            for app in apps:
                if app.id == self._device_id:
                    self._device_name = app.name
                    if self.device_expander is not None:
                        self.device_expander.set_subtitle(app.name)
                    break

        group = Adw.PreferencesGroup()
        group.set_margin_top(12)
        group.set_margin_bottom(6)

        # Offered only where the compositor can actually report focus, so it cannot be picked on a
        # desktop where it would silently never resolve.
        if self._core.is_focus_tracking_available():
            group.add(self._build_focus_row())

        for app in apps:
            row = self._build_app_row(app)
            group.add(row)
            if app.id == self._device_id:
                row.add_css_class("selected")

        # An app only appears while it holds a stream, so a configured app that is closed would
        # otherwise vanish from this list and look unset. Keep it visible and say why.
        if (
            self._device_id
            and self._device_id != FOCUSED_DEVICE_ID
            and not any(app.id == self._device_id for app in apps)
        ):
            group.add(self._build_missing_app_row())

        self.device_container.append(group)

    def _build_focus_row(self) -> Adw.ActionRow:
        """Row for the follow-the-focused-window target."""
        row = Adw.ActionRow()
        row.set_title("Focused application")
        current = self._core.focused_app_name()
        row.set_subtitle(
            f"Now: {current}" if current else "Focused window is not playing audio"
        )
        row.app_data = None
        row.focus_target = True
        row.set_activatable(True)
        row.connect("activated", self._on_focus_row_activated)
        if self._device_id == FOCUSED_DEVICE_ID:
            row.add_css_class("selected")
        return row

    def _on_focus_row_activated(self, row: Adw.ActionRow):
        self._device_id = FOCUSED_DEVICE_ID
        self._device_name = "Focused application"
        self._device_type = DeviceType.source()
        self._persist_settings(
            device_id=FOCUSED_DEVICE_ID,
            device_name=self._device_name,
            device_type=self._device_type_setting_value(self._device_type),
        )
        if self.device_expander is not None:
            self.device_expander.set_subtitle(self._device_name)
        self._update_config()

    def _build_app_row(self, app) -> Adw.ActionRow:
        row = Adw.ActionRow()
        row.set_title(app.name)
        state = "Muted" if app.is_muted else f"{app.volume}%"
        row.set_subtitle(f"{state} · {app.routed_to}" if app.routed_to else state)
        row.app_data = app
        row.set_activatable(True)
        row.connect("activated", self._on_app_row_activated)
        return row

    def _build_missing_app_row(self) -> Adw.ActionRow:
        row = Adw.ActionRow()
        row.set_title(self._device_name or str(self._device_id).removeprefix("app:"))
        row.set_subtitle("Not playing")
        row.set_sensitive(False)
        row.add_css_class("selected")
        return row

    def _on_app_row_activated(self, row: Adw.ActionRow):
        app = getattr(row, "app_data", None)
        if not app:
            return
        self._device_id = app.id
        self._device_name = app.name
        self._device_type = DeviceType.source()
        self._persist_settings(
            device_id=app.id,
            device_name=app.name,
            device_type=self._device_type_setting_value(self._device_type),
        )
        if self.device_expander is not None:
            self.device_expander.set_subtitle(app.name)
        self._update_config()


class AppKnobAction(AppActionMixin, BaseAction):
    """Per-application volume on a dial."""

    ACTION_TYPE = ActionType.knob()
    SHOW_METERS_ROW = True
    SHOW_VOLUME_ROW = True
    VOLUME_STEP_RANGE = (1, 20)

    def event_callback(self, event: Any, data: Any):
        if not self._device_id:
            return

        if event == Input.Dial.Events.TURN_CW:
            self._core.set_app_volume_relative(self._device_id, self._volume_step)
        elif event == Input.Dial.Events.TURN_CCW:
            self._core.set_app_volume_relative(self._device_id, -self._volume_step)
        elif event in (
            Input.Dial.Events.SHORT_UP,
            Input.Dial.Events.SHORT_TOUCH_PRESS,
        ):
            self._core.toggle_app_mute(self._device_id)


class AppButtonAction(AppActionMixin, ButtonAction):
    """Per-application volume on a key (positive = up, negative = down, zero = mute)."""

    def event_callback(self, event: Any, data: Any):
        self._volume_key_event(
            event,
            self._volume_step,
            self._core.set_app_volume_relative,
            self._core.toggle_app_mute,
        )


class AppSliderAction(AppActionMixin, SliderAction):
    """Per-application volume as one half of a virtual slider."""

    def event_callback(self, event: Any, data: Any):
        self._volume_key_event(
            event, self._slider_step(), self._core.set_app_volume_relative, None
        )
