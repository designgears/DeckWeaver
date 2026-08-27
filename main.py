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

# Single shared core instance
_core: Optional[DeckWeaverCore] = None
_poll_timeout_id: Optional[int] = None
_action_callbacks: dict[str, tuple[Any, Any]] = {}  # action_id -> (image_callback, label_callback)


def _poll_updates() -> bool:
    """Poll for pending updates from Rust core and process them"""
    global _core, _action_callbacks
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
    global _core, _poll_timeout_id, _action_callbacks
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
    SHOW_VOLUME_ROW = False

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
        self.device_expander: Optional[Adw.ExpanderRow] = None
        self.icon_row: Optional[Adw.ActionRow] = None
        self.icon_preview: Optional[Gtk.Image] = None
        self.output_expander: Optional[Adw.ExpanderRow] = None
        self.source_expander: Optional[Adw.ExpanderRow] = None

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
        global _action_callbacks
        self._load_settings()
        config = self._build_config()
        self._core.register_action(config)
        # Store callbacks locally for polling mechanism
        _action_callbacks[self._action_id] = (self._on_image_update, self._on_label_update)

    def _unregister_from_core(self):
        """Unregister this action from the Rust core"""
        global _action_callbacks
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
        return self._core.infer_device_type(
            device_id, isinstance(self, SourceSwitchButtonAction)
        )

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

        default_subtitle = self._icon_row_default_subtitle()
        if self._icon_path and os.path.exists(self._icon_path):
            try:
                pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(
                    self._icon_path, width=20, height=20, preserve_aspect_ratio=True
                )
                self.icon_preview.set_from_pixbuf(pixbuf)
                icon_name = os.path.splitext(os.path.basename(self._icon_path))[0]
                icon_row.set_subtitle(f"Selected: {icon_name}")
            except Exception:
                icon_row.set_subtitle(default_subtitle)
                self.icon_preview.set_visible(False)
        else:
            icon_row.set_subtitle(default_subtitle)
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
        self.icon_row = icon_row
        return icon_row

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

    def get_config_rows(self):
        """Return configuration UI rows - matching original GitHub style"""
        lm = self.plugin_base.lm
        self._load_settings()
        self.device_expander = None
        self.icon_row = None
        self.icon_preview = None

        if not self._backend_available():
            error_row = Adw.ActionRow()
            error_row.set_title(self._backend_unavailable_title())
            error_row.set_subtitle(self._backend_unavailable_subtitle())
            error_row.add_css_class("warning")
            return [error_row]

        # Device Expander Row (original style)
        self.device_expander = Adw.ExpanderRow()
        self.device_expander.set_title(self._device_picker_title())
        self.device_expander.set_subtitle(self._device_name or "No device selected")
        
        self.device_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.device_container.set_margin_start(30)
        self.device_container.set_margin_end(30)
        self.device_container.set_margin_bottom(12)
        
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_min_content_height(300)
        scrolled.set_child(self.device_container)
        
        self.device_expander.add_row(scrolled)
        self.device_expander.connect("notify::expanded", self._on_device_expander_expanded)
        self._populate_device_list()
        
        refresh_button = Gtk.Button(
            icon_name="view-refresh-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text=lm.get("ui.refresh_devices.button")
        )
        refresh_button.connect("clicked", self._on_refresh_clicked)
        self.device_expander.add_suffix(refresh_button)

        # Custom Icon Row
        icon_row = self._build_icon_row()

        min_step, max_step = self.VOLUME_STEP_RANGE
        self.volume_step_row = Adw.SpinRow.new_with_range(min_step, max_step, 1)
        self.volume_step_row.set_value(self._volume_step)
        self.volume_step_row.set_subtitle(
            self.VOLUME_STEP_SUBTITLE or lm.get("ui.volume_step.subtitle")
        )
        self.volume_step_row.set_title(lm.get("ui.volume_step.title"))
        self.volume_step_row.connect("notify::value", self._on_volume_step_changed)

        # Meters Enabled Row
        meters_enabled_row = Adw.ActionRow()
        meters_enabled_row.set_title("Meters Enabled")
        meters_enabled_row.set_subtitle("Show audio level meters")
        
        self.meters_enabled_switch = Gtk.Switch(valign=Gtk.Align.CENTER)
        self.meters_enabled_switch.set_active(self._meters_enabled)
        self.meters_enabled_switch.connect("notify::active", self._on_meters_enabled_changed)
        meters_enabled_row.add_suffix(self.meters_enabled_switch)

        # Volume Percentage Row (encoder strip only)
        show_volume_row = Adw.ActionRow()
        show_volume_row.set_title(lm.get("ui.show_volume.title"))
        show_volume_row.set_subtitle(lm.get("ui.show_volume.subtitle"))

        self.show_volume_switch = Gtk.Switch(valign=Gtk.Align.CENTER)
        self.show_volume_switch.set_active(self._show_volume)
        self.show_volume_switch.connect("notify::active", self._on_show_volume_changed)
        show_volume_row.add_suffix(self.show_volume_switch)

        rows = [
            self.device_expander,
            self.volume_step_row,
        ]
        if icon_row is not None:
            rows.insert(1, icon_row)

        if self.SHOW_METERS_ROW:
            rows.append(meters_enabled_row)

        if self.SHOW_VOLUME_ROW:
            rows.append(show_volume_row)

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
                row = self._create_device_row(device)
                group.add(row)
                if device.id == self._device_id:
                    row.add_css_class("selected")
            self.device_container.append(group)

    def _build_device_row(self, device, callback) -> Adw.ActionRow:
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
        
        return row

    def _create_device_row(self, device) -> Adw.ActionRow:
        return self._build_device_row(device, self._on_device_row_activated)

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
            try:
                pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(
                    icon_path, width=20, height=20, preserve_aspect_ratio=True
                )
                self.icon_preview.set_from_pixbuf(pixbuf)
                self.icon_preview.set_visible(True)
                if self.icon_row is not None:
                    icon_name = os.path.splitext(os.path.basename(icon_path))[0]
                    self.icon_row.set_subtitle(f"Selected: {icon_name}")
            except Exception:
                pass
        
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
        self._mute_profile_index = 0
        self._mute_profile_data: list[bool] = [False, False]

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
            if self._device_id and self._mute_profile_index < len(self._mute_profile_data):
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

    ACTION_TYPE = ActionType.button()
    VOLUME_STEP_RANGE = (-20, 20)
    VOLUME_STEP_SUBTITLE = "Positive = vol up, Negative = vol down, Zero = mute toggle"

    def get_config_rows(self):
        return super().get_config_rows()

    def _resolved_icon_path(self) -> Optional[str]:
        return super()._resolved_icon_path() or (
            DEFAULT_BUTTON_ICON_PATH if os.path.exists(DEFAULT_BUTTON_ICON_PATH) else None
        )

    def _build_hardware_device_row(self, device, callback, selected_node_id: Optional[int]):
        row = Adw.ActionRow()
        row.set_title(device.description or device.name or f"Node {device.node_id}")
        row.set_subtitle(device.name or "Physical device")
        row.device_data = device
        row.set_activatable(True)
        row.connect("activated", callback)
        if device.node_id is not None and device.node_id == selected_node_id:
            row.add_css_class("selected")
        return row

    def _populate_hardware_device_list(
        self,
        container: Gtk.Box,
        devices: list,
        callback,
        selected_node_id: Optional[int],
        empty_locale_key: str,
        empty_subtitle: str,
    ):
        self._clear_box_children(container)
        if not devices:
            row = Adw.ActionRow()
            row.set_title(
                self.plugin_base.lm.get(empty_locale_key, "No physical devices found")
            )
            row.set_subtitle(empty_subtitle)
            row.set_sensitive(False)
            container.append(row)
            return

        group = Adw.PreferencesGroup()
        group.set_margin_top(12)
        group.set_margin_bottom(6)
        for device in devices:
            row = self._build_hardware_device_row(device, callback, selected_node_id)
            group.add(row)
        container.append(group)

    def _fill_device_container(
        self,
        container: Gtk.Box,
        devices: list,
        callback,
        empty_locale_key: str,
        empty_subtitle: str,
    ):
        self._clear_box_children(container)
        if not devices:
            row = Adw.ActionRow()
            row.set_title(
                self.plugin_base.lm.get(empty_locale_key, "No devices found")
            )
            row.set_subtitle(empty_subtitle)
            row.set_sensitive(False)
            container.append(row)
            return

        group = Adw.PreferencesGroup()
        group.set_margin_top(12)
        group.set_margin_bottom(6)
        for device in devices:
            row = self._build_device_row(device, callback)
            if device.id == self._device_id:
                row.add_css_class("selected")
            group.add(row)
        container.append(group)

    def event_callback(self, event: Any, data: Any):
        if event == Input.Key.Events.SHORT_UP and self._device_id:
            if self._volume_step == 0:
                # Mute button
                self._core.toggle_mute(self._device_id, self._device_type)
            else:
                self._core.set_volume_relative(
                    self._device_id, self._volume_step, self._device_type
                )


class SourceSwitchButtonAction(ButtonAction):
    """Button that switches a hardware output target to a selected physical device."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._hardware_device_node_id: Optional[int] = None
        self._hardware_device_description: Optional[str] = None
        self._hardware_device_name: Optional[str] = None

    def _load_settings(self):
        super()._load_settings()
        settings = self.get_settings()
        self._hardware_device_description = settings.get("hardware_device_description")
        self._hardware_device_name = settings.get("hardware_device_name")

        # Resolve node_id from the stored description (which is stable across reboots)
        if self._hardware_device_description is not None and self._hardware_device_node_id is None:
            for device in self._core.get_output_hardware_devices():
                if device.description == self._hardware_device_description:
                    self._hardware_device_node_id = device.node_id
                    # Back-fill name if missing
                    if not self._hardware_device_name and device.name:
                        self._hardware_device_name = device.name
                    break

    def get_config_rows(self):
        lm = self.plugin_base.lm
        self._load_settings()
        self.output_expander = None
        self.source_expander = None

        if not self._core.is_available():
            error_row = Adw.ActionRow()
            error_row.set_title(lm.get("ui.error.not_running.title"))
            error_row.set_subtitle(lm.get("ui.error.not_running.subtitle"))
            error_row.add_css_class("warning")
            return [error_row]

        self.output_expander = Adw.ExpanderRow()
        self.output_expander.set_title(
            lm.get("ui.output_device.title", "Hardware Output Device")
        )
        self.output_expander.set_subtitle(
            self._device_name
            or lm.get("ui.output_device.none", "No hardware output selected")
        )

        self.output_device_container = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=0,
        )
        self.output_device_container.set_margin_start(30)
        self.output_device_container.set_margin_end(30)
        self.output_device_container.set_margin_bottom(12)

        output_scrolled = Gtk.ScrolledWindow()
        output_scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        output_scrolled.set_min_content_height(220)
        output_scrolled.set_child(self.output_device_container)

        self.output_expander.add_row(output_scrolled)
        self.output_expander.connect(
            "notify::expanded", self._on_output_expander_expanded
        )
        self._populate_output_device_list()

        output_refresh = Gtk.Button(
            icon_name="view-refresh-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text=lm.get("ui.refresh_devices.button"),
        )
        output_refresh.connect("clicked", self._on_output_refresh_clicked)
        self.output_expander.add_suffix(output_refresh)

        self.source_expander = Adw.ExpanderRow()
        self.source_expander.set_title(
            lm.get("ui.physical_device.title", "Physical Output Device")
        )
        self.source_expander.set_subtitle(
            self._hardware_device_name
            or lm.get("ui.physical_device.none", "No physical device selected")
        )

        self.source_device_container = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=0,
        )
        self.source_device_container.set_margin_start(30)
        self.source_device_container.set_margin_end(30)
        self.source_device_container.set_margin_bottom(12)

        source_scrolled = Gtk.ScrolledWindow()
        source_scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        source_scrolled.set_min_content_height(260)
        source_scrolled.set_child(self.source_device_container)

        self.source_expander.add_row(source_scrolled)
        self.source_expander.connect(
            "notify::expanded", self._on_source_expander_expanded
        )
        self._populate_source_device_list()

        source_refresh = Gtk.Button(
            icon_name="view-refresh-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text=lm.get("ui.refresh_devices.button"),
        )
        source_refresh.connect("clicked", self._on_source_refresh_clicked)
        self.source_expander.add_suffix(source_refresh)

        icon_row = self._build_icon_row()

        return [
            self.output_expander,
            self.source_expander,
            icon_row,
        ]

    def _build_config(self) -> ActionConfig:
        config = super()._build_config()
        config.button_overlay = False
        return config

    def _populate_output_device_list(self):
        self._fill_device_container(
            self.output_device_container,
            self._core.get_physical_targets(),
            self._on_output_row_activated,
            "ui.output_device.empty",
            "Check that PipeWeaver has detected your output device",
        )

    def _populate_source_device_list(self):
        self._populate_hardware_device_list(
            self.source_device_container,
            self._core.get_output_hardware_devices(),
            self._on_source_row_activated,
            self._hardware_device_node_id,
            "ui.physical_device.empty",
            "Check that PipeWeaver has detected your hardware outputs",
        )

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
            self.source_expander.set_subtitle(
                self._hardware_device_name
                or self.plugin_base.lm.get(
                    "ui.physical_device.none", "No physical device selected"
                )
            )

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

        self._core.switch_output_hardware_device(
            self._device_id, self._hardware_device_node_id
        )


class PhysicalSourceSwitchButtonAction(ButtonAction):
    """Button that switches a hardware input source to a selected physical device."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._hardware_device_node_id: Optional[int] = None
        self._hardware_device_description: Optional[str] = None
        self._hardware_device_name: Optional[str] = None

    def _load_settings(self):
        super()._load_settings()
        settings = self.get_settings()
        self._hardware_device_description = settings.get("hardware_device_description")
        self._hardware_device_name = settings.get("hardware_device_name")

        # Resolve node_id from the stored description (stable across reboots)
        if self._hardware_device_description is not None and self._hardware_device_node_id is None:
            for device in self._core.get_input_hardware_devices():
                if device.description == self._hardware_device_description:
                    self._hardware_device_node_id = device.node_id
                    if not self._hardware_device_name and device.name:
                        self._hardware_device_name = device.name
                    break

    def get_config_rows(self):
        lm = self.plugin_base.lm
        self._load_settings()
        self.output_expander = None
        self.source_expander = None

        if not self._core.is_available():
            error_row = Adw.ActionRow()
            error_row.set_title(lm.get("ui.error.not_running.title"))
            error_row.set_subtitle(lm.get("ui.error.not_running.subtitle"))
            error_row.add_css_class("warning")
            return [error_row]

        self.output_expander = Adw.ExpanderRow()
        self.output_expander.set_title(
            lm.get("ui.input_device.title", "Hardware Input Device")
        )
        self.output_expander.set_subtitle(
            self._device_name
            or lm.get("ui.input_device.none", "No hardware input selected")
        )

        self.output_device_container = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=0,
        )
        self.output_device_container.set_margin_start(30)
        self.output_device_container.set_margin_end(30)
        self.output_device_container.set_margin_bottom(12)

        output_scrolled = Gtk.ScrolledWindow()
        output_scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        output_scrolled.set_min_content_height(220)
        output_scrolled.set_child(self.output_device_container)

        self.output_expander.add_row(output_scrolled)
        self.output_expander.connect(
            "notify::expanded", self._on_output_expander_expanded
        )
        self._populate_output_device_list()

        output_refresh = Gtk.Button(
            icon_name="view-refresh-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text=lm.get("ui.refresh_devices.button"),
        )
        output_refresh.connect("clicked", self._on_output_refresh_clicked)
        self.output_expander.add_suffix(output_refresh)

        self.source_expander = Adw.ExpanderRow()
        self.source_expander.set_title(
            lm.get("ui.physical_input_device.title", "Physical Input Device")
        )
        self.source_expander.set_subtitle(
            self._hardware_device_name
            or lm.get("ui.physical_input_device.none", "No physical device selected")
        )

        self.source_device_container = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            spacing=0,
        )
        self.source_device_container.set_margin_start(30)
        self.source_device_container.set_margin_end(30)
        self.source_device_container.set_margin_bottom(12)

        source_scrolled = Gtk.ScrolledWindow()
        source_scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        source_scrolled.set_min_content_height(260)
        source_scrolled.set_child(self.source_device_container)

        self.source_expander.add_row(source_scrolled)
        self.source_expander.connect(
            "notify::expanded", self._on_source_expander_expanded
        )
        self._populate_source_device_list()

        source_refresh = Gtk.Button(
            icon_name="view-refresh-symbolic",
            valign=Gtk.Align.CENTER,
            tooltip_text=lm.get("ui.refresh_devices.button"),
        )
        source_refresh.connect("clicked", self._on_source_refresh_clicked)
        self.source_expander.add_suffix(source_refresh)

        icon_row = self._build_icon_row()

        return [
            self.output_expander,
            self.source_expander,
            icon_row,
        ]

    def _build_config(self) -> ActionConfig:
        config = super()._build_config()
        config.button_overlay = False
        return config

    def _populate_output_device_list(self):
        self._fill_device_container(
            self.output_device_container,
            self._core.get_physical_sources(),
            self._on_output_row_activated,
            "ui.input_device.empty",
            "Check that PipeWeaver has detected your input device",
        )

    def _populate_source_device_list(self):
        self._populate_hardware_device_list(
            self.source_device_container,
            self._core.get_input_hardware_devices(),
            self._on_source_row_activated,
            self._hardware_device_node_id,
            "ui.physical_input_device.empty",
            "Check that PipeWeaver has detected your hardware inputs",
        )

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
            self.source_expander.set_subtitle(
                self._hardware_device_name
                or self.plugin_base.lm.get(
                    "ui.physical_input_device.none", "No physical device selected"
                )
            )

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

        self._core.switch_input_hardware_device(
            self._device_id, self._hardware_device_node_id
        )


class SliderAction(BaseAction):
    """Slider button (top/bottom half of virtual slider)"""

    ACTION_TYPE = ActionType.slider()
    VOLUME_STEP_RANGE = (-20, 20)
    VOLUME_STEP_SUBTITLE = "Positive = top slider, Negative = bottom"
    SHOW_METERS_ROW = True

    def _build_config(self) -> ActionConfig:
        config = super()._build_config()
        settings = self.get_settings()
        config.orientation = settings.get("orientation", "vertical")
        config.is_top = self._volume_step > 0
        return config

    def get_config_rows(self):
        # Base class already creates the correct row for SliderAction
        rows = super().get_config_rows()

        # Orientation selector
        lm = self.plugin_base.lm
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
        if event == Input.Key.Events.SHORT_UP and self._device_id:
            self._core.set_volume_relative(
                self._device_id, self._volume_step, self._device_type
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


class AppButtonAction(AppActionMixin, BaseAction):
    """Per-application volume on a key (positive = up, negative = down, zero = mute)."""

    ACTION_TYPE = ActionType.button()
    VOLUME_STEP_RANGE = (-20, 20)
    VOLUME_STEP_SUBTITLE = "Positive = vol up, Negative = vol down, Zero = mute toggle"

    def _resolved_icon_path(self) -> Optional[str]:
        return super()._resolved_icon_path() or (
            DEFAULT_BUTTON_ICON_PATH if os.path.exists(DEFAULT_BUTTON_ICON_PATH) else None
        )

    def event_callback(self, event: Any, data: Any):
        if event == Input.Key.Events.SHORT_UP and self._device_id:
            if self._volume_step == 0:
                self._core.toggle_app_mute(self._device_id)
            else:
                self._core.set_app_volume_relative(self._device_id, self._volume_step)


class AppSliderAction(AppActionMixin, BaseAction):
    """Per-application volume as one half of a virtual slider."""

    ACTION_TYPE = ActionType.slider()
    VOLUME_STEP_RANGE = (-20, 20)
    VOLUME_STEP_SUBTITLE = "Positive = top slider, Negative = bottom"
    SHOW_METERS_ROW = True

    def _build_config(self) -> ActionConfig:
        config = super()._build_config()
        settings = self.get_settings()
        config.orientation = settings.get("orientation", "vertical")
        config.is_top = self._volume_step > 0
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
        if event == Input.Key.Events.SHORT_UP and self._device_id:
            self._core.set_app_volume_relative(self._device_id, self._volume_step)
