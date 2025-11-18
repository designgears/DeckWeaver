"""Knob-specific action for PipeWeaver"""
from src.backend.DeckManagement.InputIdentifier import Input  # type: ignore
from .action_base import PipeWeaverAction
import threading
from loguru import logger as log


class PipeWeaverKnobAction(PipeWeaverAction):
    """Knob-specific action: Volume on turn, bus selection and linking on touchscreen"""
    
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._menu_mode = False
        self._menu_timer = None
        self._menu_timeout = 5.0
    
    def event_callback(self, event, data):
        """Handle input events for knobs"""
        event_str = str(event)
        
        if event_str == "Dial Touchscreen Long Press":
            if self._menu_mode:
                self._menu_mode = False
                self.update_image()
            return
        elif event_str == "Dial Touchscreen Short Press":
            if self._menu_mode:
                self._handle_menu_touch(event_str, data)
            else:
                self._toggle_menu()
            return
        elif event_str == "Touchscreen Drag Left":
            return
        elif event_str == "Touchscreen Drag Right":
            return
        
        if self._menu_mode and ("Touchscreen" in event_str or "Dial Touchscreen" in event_str):
            self._handle_menu_touch(event_str, data)
            return
        
        if event_str == "Dial Up":
            return
        
        if event == Input.Dial.Events.TURN_CW:
            volume_step = getattr(self, 'volume_step', 5)
            self._set_volume_relative(volume_step)
        elif event == Input.Dial.Events.TURN_CCW:
            volume_step = getattr(self, 'volume_step', 5)
            self._set_volume_relative(-volume_step)
        elif event_str == "Dial Short Up":
            self._toggle_mute()
    
    def _cycle_bus_forward(self):
        """Cycle bus selection forward: A -> B -> Both -> A"""
        if self._cycling_bus or self.selected_device_type != "source":
            return
        
        if self.selected_device_id and self.client.is_volume_linked(self.selected_device_id):
            if "B" not in self.selected_mixes:
                self._update_mixes(["B"])
            return
        
        if self._have_different_mute_states():
            self._update_mixes(["A"])
            return
        
        self._cycling_bus = True
        
        try:
            current_mixes = set(self.selected_mixes)
            
            if current_mixes == {"A"}:
                new_mixes = ["B"]
            elif current_mixes == {"B"}:
                new_mixes = ["A", "B"] if not self._have_different_mute_states() else ["A"]
            elif current_mixes == {"A", "B"}:
                new_mixes = ["A"]
            else:
                new_mixes = ["A"]
            
            self._update_mixes(new_mixes)
        finally:
            self._cycling_bus = False
    
    def _update_mixes(self, mixes):
        """Update mix selections and save settings"""
        self.selected_mixes = set(mixes)
        settings = self.get_settings()
        settings["selected_mixes"] = mixes
        self.set_settings(settings)
        self.update_image()
    
    def _toggle_menu(self):
        """Toggle the menu mode on/off"""
        if self._menu_mode:
            self._close_menu()
        else:
            self._menu_mode = True
            self._start_menu_timer()
            self.update_image()
    
    def _start_menu_timer(self):
        """Start or restart the menu timeout timer"""
        if self._menu_timer:
            self._menu_timer.cancel()
        
        self._menu_timer = threading.Timer(self._menu_timeout, self._close_menu)
        self._menu_timer.start()
    
    def _close_menu(self):
        """Close the menu and cancel timer"""
        if self._menu_timer:
            self._menu_timer.cancel()
            self._menu_timer = None
        
        if self._menu_mode:
            self._menu_mode = False
            self.update_image()
    
    def _handle_menu_touch(self, event_str, data):
        """Handle touch events when menu is active"""
        try:
            self._start_menu_timer()
            
            x, y = None, None
            if hasattr(data, 'x') and hasattr(data, 'y'):
                x, y = data.x, data.y
            elif isinstance(data, dict):
                if 'x' in data and 'y' in data:
                    x, y = data['x'], data['y']
                elif 'coords' in data:
                    x, y = data['coords']
            elif isinstance(data, (list, tuple)) and len(data) >= 2:
                x, y = data[0], data[1]
            
            if x is None or y is None:
                self._close_menu()
                return
            
            section_width = 200
            x_in_section = x % section_width
            
            touch_min_x, touch_max_x = 0, section_width
            display_min_x, display_max_x = 0, 480
            
            if x_in_section < touch_min_x:
                mapped_x = display_min_x
            elif x_in_section > touch_max_x:
                mapped_x = display_max_x
            else:
                touch_range = touch_max_x - touch_min_x
                display_range = display_max_x - display_min_x
                mapped_x = int(((x_in_section - touch_min_x) / touch_range) * display_range)
            
            touch_min_y, touch_max_y = 70, 90
            display_min_y, display_max_y = 120, 225
            
            if y < touch_min_y or y > touch_max_y:
                mapped_y = -1
            else:
                touch_range_y = touch_max_y - touch_min_y
                display_range_y = display_max_y - display_min_y
                mapped_y = display_min_y + int(((y - touch_min_y) / touch_range_y) * display_range_y)
            
            if hasattr(self, '_image_renderer'):
                for i, button in enumerate(self._image_renderer._menu_buttons):
                    x_min = button['x']
                    x_max = button['x'] + button['width']
                    y_min = button['y']
                    y_max = button['y'] + button['height']
                    
                    x_in_range = x_min <= mapped_x <= x_max
                    y_in_range = y_min <= mapped_y <= y_max
                    
                    if x_in_range and y_in_range:
                        self._execute_menu_action(button['action'])
                        return
            
            self._close_menu()
            
        except Exception as e:
            log.error(f"Error handling menu touch: {e}")
            self._close_menu()
    
    def _execute_menu_action(self, action):
        """Execute the selected menu action"""
        try:
            if action == "link":
                self._toggle_volume_linking()
            elif action == "unlink":
                self._toggle_volume_linking()
            elif action == "bus_a":
                self._toggle_bus_selection("A")
            elif action == "bus_b":
                self._toggle_bus_selection("B")
            else:
                log.warning(f"Unknown menu action: {action}")
            
            if self._menu_mode:
                self.update_image()
            
        except Exception as e:
            log.error(f"Error executing menu action {action}: {e}")
    
    def _toggle_bus_selection(self, bus):
        """Toggle selection of a specific bus"""
        if self.selected_device_type != "source":
            return
        
        if (self.selected_device_id and 
            self.client.is_volume_linked(self.selected_device_id) and 
            bus != "B"):
            return
        
        current_mixes = set(self.selected_mixes)
        
        if bus in current_mixes:
            current_mixes.discard(bus)
        else:
            current_mixes.add(bus)
        
        if not current_mixes:
            current_mixes = {"A"} if bus == "B" else {"B"}
        
        self._update_mixes(list(current_mixes))
    
    def _have_different_mute_states(self):
        """Check if buses A and B have different mute states"""
        if not self.selected_device_id or self.selected_device_type != "source":
            return False
        
        try:
            mix_states, _ = self._get_source_mix_states(["A", "B"])
            a_muted = mix_states.get("A", False)
            b_muted = mix_states.get("B", False)
            return a_muted != b_muted
        except Exception as e:
            log.error(f"Error checking mute states: {e}")
            return False
    
    
