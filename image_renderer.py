"""Image rendering utilities for PipeWeaver actions"""
import os
import sys
import traceback

from PIL import Image, ImageDraw, ImageFont  # type: ignore
from loguru import logger as log  # type: ignore


class ImageRenderer:
    """Renders images for PipeWeaver actions using PIL"""
    
    def __init__(self, action):
        """Initialize renderer with action instance"""
        self.action = action
    
    def render_image(self):
        """Render the button image - shows mute state or volume bars"""
        if not self.action.selected_device_name:
            display_text = self.action.selected_device_name if self.action.selected_device_name else "PipeWeaver"
            if hasattr(self.action, 'set_label'):
                self.action.set_label(text=display_text, position="center", font_size=10)
            return

        self.action._verify_and_update_device_id()
        
        device_short = self.action.selected_device_name[:7] if self.action.selected_device_name else ""

        muted = False
        device_data = None
        is_linked = False
        
        try:
            if self.action.selected_device_type == "source":
                device_data = self.action._get_device_by_id(self.action.selected_device_id, "source")
                if device_data:
                    mute_states = device_data.get("mute_states", {}).get("mute_state", [])
                    selected_mixes = list(self.action.selected_mixes)
                    
                    is_a_muted = "TargetA" in mute_states
                    is_b_muted = "TargetB" in mute_states
                    
                    is_linked = False
                    volumes_dict = device_data.get("volumes", {})
                    if isinstance(volumes_dict, dict):
                        volumes_linked = volumes_dict.get("volumes_linked")
                        is_linked = volumes_linked is not None
                    
                    if is_linked:
                        muted = is_b_muted
                        is_a_selected = "A" in self.action.selected_mixes
                        is_b_selected = "B" in self.action.selected_mixes
                        if is_b_selected:
                            is_a_muted = is_b_muted
                    else:
                        muted = any(f"Target{mix}" in mute_states for mix in selected_mixes)
            else:
                device_data = self.action._get_device_by_id(self.action.selected_device_id, "target")
                if device_data:
                    muted = device_data.get("mute_state") == "Muted"
                    is_a_muted = muted
                    is_b_muted = muted
        except Exception as e:
            log.error(f"Error getting mute state: {e}")
            muted = False
            is_a_muted = False
            is_b_muted = False
        
        try:
            if hasattr(self.action, '_menu_mode') and self.action._menu_mode:
                image = self._render_menu()
            elif self.action.selected_device_type == "source":
                image = self._render_source_device(device_data, muted, device_short, is_a_muted, is_b_muted)
            else:
                image = self._render_target_device(device_data, muted, device_short)
            
            if image:
                self._set_image_on_action(image, device_short)
        except Exception as e:
            log.error(f"Error drawing volume bars: {e}")
            log.error(traceback.format_exc())
    
    def _load_monospace_font(self, size=12):
        """Load a bold, clean monospace font with fallback to default"""
        font_paths = [
            "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Bold.ttf",
            "/usr/share/fonts/truetype/source-code-pro/SourceCodePro-Bold.ttf", 
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Bold.ttf",
            "/usr/share/fonts/truetype/ubuntu/UbuntuMono-Bold.ttf",
            "/usr/share/fonts/truetype/fira-code/FiraCode-Bold.ttf",
            "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
            "/usr/share/fonts/truetype/source-code-pro/SourceCodePro-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
            "/usr/share/fonts/TTF/arial.ttf",
            "/System/Library/Fonts/Monaco.ttf",
            "C:/Windows/Fonts/consola.ttf",
        ]
        
        for font_path in font_paths:
            try:
                return ImageFont.truetype(font_path, size)
            except:
                continue
        
        return ImageFont.load_default()
    
    def _render_source_device(self, device_data, muted, device_short, is_a_muted=False, is_b_muted=False):
        """Render image for source device with two volume bars"""
        if device_data:
            try:
                volumes_dict = device_data.get("volumes", {})
                if isinstance(volumes_dict, dict):
                    volume_dict = volumes_dict.get("volume", {})
                    if isinstance(volume_dict, dict):
                        volume_a_raw = volume_dict.get("A", 0)
                        volume_b_raw = volume_dict.get("B", 0)
                        
                        if volume_a_raw > 100:
                            volume_a = int((volume_a_raw / 255.0) * 100)
                        else:
                            volume_a = volume_a_raw
                        
                        if volume_b_raw > 100:
                            volume_b = int((volume_b_raw / 255.0) * 100)
                        else:
                            volume_b = volume_b_raw
                        
                        volumes = [volume_a, volume_b]
                    else:
                        volumes = [0, 0]
                else:
                    volumes = [0, 0]
            except Exception as vol_e:
                log.error(f"Error accessing volume data: {vol_e}")
                volumes = [0, 0]
        else:
            volumes = [0, 0]
        
        try:
            is_linked = False
            if device_data:
                volumes_dict = device_data.get("volumes", {})
                if isinstance(volumes_dict, dict):
                    volumes_linked = volumes_dict.get("volumes_linked")
                    is_linked = volumes_linked is not None
            
            image_width = 480
            image_height = 240
            
            image = Image.new('RGBA', (image_width, image_height), (0, 0, 0, 0))
            draw = ImageDraw.Draw(image)
            
            try:
                font = self._load_monospace_font(34)
            except:
                try:
                    font = self._load_monospace_font(34)
                except:
                    font = ImageFont.load_default()
            
            edge_padding = 10
            
            icon_max_size = 150
            icon_bottom_y = image_height - icon_max_size - edge_padding
            icon_left_x = edge_padding
            
            left_margin = icon_max_size + edge_padding + edge_padding
            right_margin = edge_padding
            bar_width = image_width - left_margin - right_margin
            bar_height = 24
            bar_spacing = 12
            start_x = left_margin
            bar_b_y = image_height - bar_height - edge_padding - 15
            bar_a_y = bar_b_y - bar_height - bar_spacing
            
            device_name = self.action.selected_device_name[:25] if self.action.selected_device_name else "Unknown"
            
            draw.text((edge_padding, edge_padding), device_name, fill=(255, 255, 255, 255), font=font)

            try:
                label_font = self._load_monospace_font(12)
            except:
                label_font = ImageFont.load_default()

            draw.text((start_x + 4, bar_a_y - 15), "VOL", fill=(204, 204, 204, 204), font=label_font)

            meter_label_y = bar_a_y + bar_height + 12
            draw.text((start_x + 4, meter_label_y), "LVL", fill=(204, 204, 204, 204), font=label_font)

            volume_a = volumes[0] if len(volumes) > 0 else 0
            volume_b = volumes[1] if len(volumes) > 1 else 0
            
            if is_linked:
                pass
            
            bar_a_fill_width = int((volume_a / 100.0) * bar_width)
            bar_b_fill_width = int((volume_b / 100.0) * bar_width)
            
            is_a_selected = "A" in self.action.selected_mixes
            is_b_selected = "B" in self.action.selected_mixes

            indicators_y = bar_a_y - 45

            total_indicators = (1 if is_linked else 0) + (1 if is_a_selected else 0) + (1 if is_b_selected else 0)

            if total_indicators > 0:
                indicator_index = 0

                icon_size = 48
                icon_x = start_x + 4
                icon_y = indicators_y - 4

                if is_linked:
                    icon_name = "linked-white.png"
                else:
                    icon_name = "unlinked-dimmed.png"
                
                icon_path = self.action.plugin_base.get_asset_path(icon_name, ["icons"])
                if os.path.exists(icon_path):
                    link_icon = Image.open(icon_path)
                    if link_icon.mode == 'P':
                        link_icon = link_icon.convert('RGBA')
                    link_icon_resized = link_icon.resize((icon_size, icon_size), Image.Resampling.LANCZOS)
                    if link_icon_resized.mode != 'RGBA':
                        link_icon_resized = link_icon_resized.convert('RGBA')
                    image.paste(link_icon_resized, (icon_x, icon_y), link_icon_resized)
                indicator_index += 1

            self._draw_unlinked_bars(draw, start_x, bar_width, bar_a_y, bar_b_y, bar_height,
                                    bar_a_fill_width, bar_b_fill_width, is_a_selected, is_b_selected, 
                                    is_a_muted, is_b_muted)

            self._composite_icon(image, icon_left_x, icon_bottom_y, icon_max_size)

            return image
        except Exception as img_e:
            log.error(f"Error creating image: {img_e}")
            log.error(traceback.format_exc())
            return None
    
    def _render_target_device(self, device_data, muted, device_short):
        """Render image for target device with single volume bar"""
        if device_data:
            volume_raw = device_data.get("volume", 0)
            if volume_raw > 100:
                volume = int((volume_raw / 255.0) * 100)
            else:
                volume = volume_raw
        else:
            volume = 0
        
        try:
            image_width = 480
            image_height = 240
            
            image = Image.new('RGBA', (image_width, image_height), (0, 0, 0, 0))
            draw = ImageDraw.Draw(image)
            
            try:
                font = self._load_monospace_font(34)
            except:
                try:
                    font = self._load_monospace_font(34)
                except:
                    font = ImageFont.load_default()
            
            edge_padding = 10
            
            icon_max_size = 150
            icon_bottom_y = image_height - icon_max_size - edge_padding
            icon_left_x = edge_padding
            
            left_margin = icon_max_size + edge_padding + edge_padding
            right_margin = edge_padding
            bar_width = image_width - left_margin - right_margin
            bar_height = 24
            bar_x = left_margin
            bar_y = image_height - bar_height - edge_padding - 15
            
            display_volume = volume
            bar_fill_width = int((display_volume / 100.0) * bar_width)

            device_name = self.action.selected_device_name[:25] if self.action.selected_device_name else "Unknown"
            
            draw.text((edge_padding, edge_padding), device_name, fill=(255, 255, 255, 255), font=font)

            try:
                label_font = self._load_monospace_font(12)
            except:
                label_font = ImageFont.load_default()

            draw.text((bar_x + 4, bar_y - 15), "VOL", fill=(204, 204, 204, 204), font=label_font)

            meter_label_y = bar_y + bar_height + 12
            draw.text((bar_x + 4, meter_label_y), "LVL", fill=(204, 204, 204, 204), font=label_font)

            if muted:
                bg_color = (38, 38, 38, 255)
                outline_color = (77, 77, 77, 255)
            else:
                bg_color = (20, 38, 20, 255)
                outline_color = (102, 204, 102, 255)

            radius = bar_height // 2

            self._draw_rounded_rect(draw, (bar_x, bar_y, bar_x + bar_width, bar_y + bar_height), radius, bg_color)

            self._draw_rounded_rect_outline(draw, (bar_x, bar_y, bar_x + bar_width, bar_y + bar_height), radius, outline_color, 2)

            if bar_fill_width > 0:
                fill_color = (77, 77, 77, 255) if muted else (102, 255, 102, 255)
                fill_x1 = bar_x + 2
                fill_x2 = bar_x + min(bar_fill_width, bar_width - 2)
                fill_y1 = bar_y + 2
                fill_y2 = bar_y + bar_height - 2
                if fill_x2 > fill_x1:
                    self._draw_rounded_rect(draw, (fill_x1, fill_y1, fill_x2, fill_y2), max(0, radius - 2), fill_color)
            
            meter_value = self.action._current_meter_target
            if meter_value > 0 and bar_fill_width > 0:
                self._draw_animated_meter(draw, meter_value, bar_fill_width, bar_x, bar_width,
                                        bar_y + bar_height - 9, 6, radius)
            
            self._composite_icon(image, icon_left_x, icon_bottom_y, icon_max_size)

            return image
        except Exception as img_e:
            log.error(f"Error creating image: {img_e}")
            log.error(traceback.format_exc())
            return None
    
    def _draw_rounded_rect(self, draw, bbox, radius, fill):
        """Draw a rounded rectangle"""
        x1, y1, x2, y2 = bbox
        if radius <= 0:
            draw.rectangle(bbox, fill=fill)
            return
        
        draw.rectangle([x1 + radius, y1, x2 - radius, y2], fill=fill)
        draw.rectangle([x1, y1 + radius, x2, y2 - radius], fill=fill)
        
        draw.pieslice([x1, y1, x1 + 2*radius, y1 + 2*radius], 180, 270, fill=fill)
        draw.pieslice([x2 - 2*radius, y1, x2, y1 + 2*radius], 270, 360, fill=fill)
        draw.pieslice([x1, y2 - 2*radius, x1 + 2*radius, y2], 90, 180, fill=fill)
        draw.pieslice([x2 - 2*radius, y2 - 2*radius, x2, y2], 0, 90, fill=fill)
    
    def _draw_rounded_rect_outline(self, draw, bbox, radius, outline, width=1):
        """Draw a rounded rectangle outline"""
        x1, y1, x2, y2 = bbox
        if radius <= 0:
            draw.rectangle(bbox, outline=outline, width=width)
            return
        
        draw.rectangle([x1 + radius, y1, x2 - radius, y1 + width], fill=outline)
        draw.rectangle([x1 + radius, y2 - width, x2 - radius, y2], fill=outline)
        draw.rectangle([x1, y1 + radius, x1 + width, y2 - radius], fill=outline)
        draw.rectangle([x2 - width, y1 + radius, x2, y2 - radius], fill=outline)
        
        if width == 1:
            draw.arc([x1, y1, x1 + 2*radius, y1 + 2*radius], 180, 270, fill=outline)
            draw.arc([x2 - 2*radius, y1, x2, y1 + 2*radius], 270, 360, fill=outline)
            draw.arc([x1, y2 - 2*radius, x1 + 2*radius, y2], 90, 180, fill=outline)
            draw.arc([x2 - 2*radius, y2 - 2*radius, x2, y2], 0, 90, fill=outline)
        else:
            for i in range(width):
                offset = i
                draw.arc([x1 - offset, y1 - offset, x1 + 2*radius + offset, y1 + 2*radius + offset], 180, 270, fill=outline)
                draw.arc([x2 - 2*radius - offset, y1 - offset, x2 + offset, y1 + 2*radius + offset], 270, 360, fill=outline)
                draw.arc([x1 - offset, y2 - 2*radius - offset, x1 + 2*radius + offset, y2 + offset], 90, 180, fill=outline)
                draw.arc([x2 - 2*radius - offset, y2 - 2*radius - offset, x2 + offset, y2 + offset], 0, 90, fill=outline)
    
    def _draw_unlinked_bars(self, draw, start_x, bar_width, bar_a_y, bar_b_y, bar_height,
                           bar_a_fill_width, bar_b_fill_width, is_a_selected, is_b_selected, 
                           is_a_muted, is_b_muted):
        """Draw unlinked volume bars (two separate bars) with enhanced visibility"""
        if is_a_muted:
            bg_color_a = (38, 38, 38, 255)
            outline_color_a = (102, 153, 255, 255) if is_a_selected else (77, 77, 77, 255)
        else:
            bg_color_a = (26, 26, 51, 255)
            outline_color_a = (102, 153, 255, 255) if is_a_selected else (51, 77, 128, 255)

        if is_b_muted:
            bg_color_b = (38, 38, 38, 255)
            outline_color_b = (255, 153, 51, 255) if is_b_selected else (77, 77, 77, 255)
        else:
            bg_color_b = (51, 26, 13, 255)
            outline_color_b = (255, 153, 51, 255) if is_b_selected else (128, 77, 26, 255)

        radius = bar_height // 2

        draw.rectangle([start_x, bar_a_y, start_x + bar_width, bar_a_y + bar_height], fill=bg_color_a)
        draw.rectangle([start_x, bar_a_y, start_x + bar_width, bar_a_y + bar_height], outline=outline_color_a, width=4)

        if bar_a_fill_width > 0:
            fill_color_a = (77, 77, 77, 255) if is_a_muted else (102, 179, 255, 255)
            fill_x1 = start_x + 4
            fill_x2 = start_x + min(bar_a_fill_width, bar_width - 4)
            fill_y1 = bar_a_y + 4
            fill_y2 = bar_a_y + bar_height - 4
            if fill_x2 > fill_x1:
                draw.rectangle([fill_x1, fill_y1, fill_x2, fill_y2], fill=fill_color_a)

        draw.rectangle([start_x, bar_b_y, start_x + bar_width, bar_b_y + bar_height], fill=bg_color_b)
        draw.rectangle([start_x, bar_b_y, start_x + bar_width, bar_b_y + bar_height], outline=outline_color_b, width=4)

        if bar_b_fill_width > 0:
            fill_color_b = (77, 77, 77, 255) if is_b_muted else (255, 179, 77, 255)
            fill_x1 = start_x + 4
            fill_x2 = start_x + min(bar_b_fill_width, bar_width - 4)
            fill_y1 = bar_b_y + 4
            fill_y2 = bar_b_y + bar_height - 4
            if fill_x2 > fill_x1:
                draw.rectangle([fill_x1, fill_y1, fill_x2, fill_y2], fill=fill_color_b)

        self._draw_unlinked_meters(draw, start_x, bar_width, bar_a_y, bar_b_y, bar_height,
                                  bar_a_fill_width, bar_b_fill_width, radius)
    
    def _draw_unlinked_meters(self, draw, start_x, bar_width, bar_a_y, bar_b_y, bar_height,
                             bar_a_fill_width, bar_b_fill_width, radius):
        """Draw animated meter overlays for unlinked volume bars"""
        meter_a = self.action._current_meter_a
        meter_b = self.action._current_meter_b

        if meter_a > 0 and bar_a_fill_width > 0:
            self._draw_animated_meter(draw, meter_a, bar_a_fill_width, start_x, bar_width,
                                    bar_a_y + bar_height - 9, 6, radius)

        if meter_b > 0 and bar_b_fill_width > 0:
            self._draw_animated_meter(draw, meter_b, bar_b_fill_width, start_x, bar_width,
                                    bar_b_y + bar_height - 9, 6, radius)
    
    def _draw_animated_meter(self, draw, meter_value, fill_width, start_x, bar_width, meter_y, meter_height, radius):
        """Draw simple black meter bars"""
        if meter_value <= 0 or fill_width <= 0:
            return

        base_meter_width = int((meter_value / 100.0) * fill_width)
        meter_x1 = start_x
        meter_x2 = start_x + base_meter_width

        if meter_x2 <= meter_x1 or meter_y < 0:
            return

        edge_inset = 6
        meter_x1_inset = max(meter_x1, start_x + edge_inset)
        meter_x2_inset = min(meter_x2, start_x + bar_width - edge_inset)

        if meter_x2_inset > meter_x1_inset:
            draw.rectangle([meter_x1_inset, meter_y, meter_x2_inset, meter_y + meter_height], fill=(0, 0, 0, 255))

    def _composite_icon(self, image, icon_left_x, icon_bottom_y, icon_max_size):
        """Composite icon onto image if configured"""
        try:
            icon = self.action._get_icon()
            if icon and isinstance(icon, Image.Image):
                icon_w, icon_h = icon.size
                scale = min(icon_max_size / icon_w, icon_max_size / icon_h, 1.0)
                icon_size = (int(icon_w * scale), int(icon_h * scale))
                icon_resized = icon.resize(icon_size, Image.Resampling.LANCZOS)
                if icon_resized.mode != 'RGBA':
                    icon_resized = icon_resized.convert('RGBA')

                final_icon_bottom_y = icon_bottom_y + (icon_max_size - icon_size[1])
                final_icon_left_x = icon_left_x + (icon_max_size - icon_size[0]) // 2
                
                image.paste(icon_resized, (final_icon_left_x, final_icon_bottom_y), icon_resized)
        except Exception as e:
            log.warning(f"Error compositing icon: {e}")
            pass
    
    def _render_menu(self):
        """Render the interactive menu with 3 horizontal full-screen buttons"""
        image_width = 480
        image_height = 240
        image = Image.new("RGBA", (image_width, image_height), (0, 0, 0, 0))
        draw = ImageDraw.Draw(image)
        
        button_font = self._load_monospace_font(64)
        
        margin = 15
        total_margins = margin * 4
        button_width = (image_width - total_margins) // 3
        button_height = (image_height - (margin * 2)) // 2
        start_x = margin
        start_y = image_height - button_height - margin
        
        buttons = [
            ("Link", "link", (100, 200, 100)),
            ("A", "bus_a", (102, 179, 255)),
            ("B", "bus_b", (255, 179, 77))
        ]
        
        for i, (label, action_key, color) in enumerate(buttons):
            x = start_x + i * (button_width + margin)
            y = start_y
            
            if action_key == "link" and self.action.selected_device_id:
                is_linked = self.action.client.is_volume_linked(self.action.selected_device_id)
                if is_linked:
                    label = "Unlink"
                else:
                    label = "Link"
            
            is_selected = False
            if action_key == "bus_a" and "A" in self.action.selected_mixes:
                is_selected = True
            elif action_key == "bus_b" and "B" in self.action.selected_mixes:
                is_selected = True
            elif action_key == "link" and self.action.selected_device_id:
                is_linked = self.action.client.is_volume_linked(self.action.selected_device_id)
                is_selected = is_linked
            
            if action_key == "bus_a":
                if is_selected:
                    bg_color = color + (255,)
                else:
                    bg_color = tuple(int(c * 0.6) for c in color) + (255,)
                outline_color = (51, 77, 128, 255)
            elif action_key == "bus_b":
                if is_selected:
                    bg_color = color + (255,)
                else:
                    bg_color = tuple(int(c * 0.6) for c in color) + (255,)
                outline_color = (128, 77, 26, 255)
            else:
                if is_selected:
                    bg_color = color + (255,)
                else:
                    bg_color = tuple(int(c * 0.6) for c in color) + (255,)
                outline_color = tuple(int(c * 0.9) for c in color) + (255,)
            
            radius = 10
            draw.rounded_rectangle([x, y, x + button_width, y + button_height], 
                                  radius=radius, fill=bg_color, outline=outline_color, width=2)
            
            if action_key == "link":
                icon_size = min(button_width - 20, button_height - 20)
                icon_x = x + (button_width - icon_size) // 2
                icon_y = y + (button_height - icon_size) // 2
                
                if self.action.selected_device_id:
                    is_linked = self.action.client.is_volume_linked(self.action.selected_device_id)
                    icon_name = "linked-white.png" if is_linked else "unlinked-dimmed.png"
                else:
                    icon_name = "unlinked-dimmed.png"
                
                icon_path = self.action.plugin_base.get_asset_path(icon_name, ["icons"])
                if os.path.exists(icon_path):
                    link_icon = Image.open(icon_path)
                    if link_icon.mode == 'P':
                        link_icon = link_icon.convert('RGBA')
                    elif link_icon.mode != 'RGBA':
                        link_icon = link_icon.convert('RGBA')
                    link_icon_resized = link_icon.resize((icon_size, icon_size), Image.Resampling.LANCZOS)
                    image.paste(link_icon_resized, (icon_x, icon_y), link_icon_resized)
            else:
                text_bbox = draw.textbbox((0, 0), label, font=button_font)
                text_width = text_bbox[2] - text_bbox[0]
                text_height = text_bbox[3] - text_bbox[1]
                text_x = x + (button_width - text_width) // 2
                text_y = y + (button_height - text_height) // 2 - text_bbox[1]
                
                draw.text((text_x, text_y), label, fill=(255, 255, 255, 255), font=button_font)
        
        self._menu_buttons = []
        for i, (label, action_key, color) in enumerate(buttons):
            x = start_x + i * (button_width + margin)
            y = start_y
            button_info = {
                'x': x,
                'y': y,
                'width': button_width,
                'height': button_height,
                'action': action_key
            }
            self._menu_buttons.append(button_info)
        
        return image
    
    def _set_image_on_action(self, image, device_short):
        """Set the rendered image on the action"""
        try:
            if hasattr(self.action, 'set_label'):
                self.action.set_label(None)
            if hasattr(self.action, 'set_bottom_label'):
                self.action.set_bottom_label(None)
            if hasattr(self.action, 'set_top_label'):
                self.action.set_top_label(None)

            self.action.set_media(image=image)
        except Exception as e:
            log.error(f"Error setting image: {e}")
            log.error(traceback.format_exc())
