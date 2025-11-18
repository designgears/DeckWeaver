"""WebSocket client for communicating with PipeWeaver daemon"""
import json
import threading
import traceback
import time
import os
import sys
from queue import Queue, Empty
from loguru import logger as log  # type: ignore

_vendor_path = os.path.join(os.path.dirname(__file__), 'vendor')
if os.path.exists(_vendor_path) and _vendor_path not in sys.path:
    sys.path.insert(0, _vendor_path)

try:
    import websocket  # type: ignore
except ImportError:
    log.error("websocket-client library not found. It should be bundled in the vendor/ directory.")
    websocket = None

try:
    import jsonpatch  # type: ignore
except ImportError:
    log.error("jsonpatch library not found. It should be bundled in the vendor/ directory.")
    jsonpatch = None


class MeterWebSocketClient:
    """WebSocket client for receiving meter data"""
    
    def __init__(self, callback, port=14565):
        if websocket is None:
            raise ImportError("websocket-client library is required. Install it with: pip install websocket-client")
        
        self.callback = callback
        self.port = port
        self.ws = None
        self.running = False
        self.thread = None
    
    def _run(self):
        """Run WebSocket client in thread using websocket-client library"""
        while self.running:
            try:
                url = f"ws://localhost:{self.port}/api/websocket/meter"

                self.ws = websocket.create_connection(url, timeout=5)
                
                while self.running:
                    try:
                        self.ws.sock.settimeout(1.0)
                        message = self.ws.recv()
                        if message:
                            try:
                                data = json.loads(message)
                                if 'id' in data and 'percent' in data:
                                    node_id = str(data['id'])
                                    percent = int(data['percent'])
                                    self.callback(node_id, percent)
                                else:
                                    log.warning(f"Meter message missing id or percent: {data}")
                            except json.JSONDecodeError as e:
                                log.warning(f"Failed to parse meter message: {e}")
                    except websocket.WebSocketTimeoutException:
                        continue
                    except websocket.WebSocketConnectionClosedException:
                        log.warning("Meter WebSocket connection closed")
                        break
                    except Exception as e:
                        log.error(f"Error receiving meter message: {e}")
                        log.error(traceback.format_exc())
                        break
                        
            except Exception as e:
                if self.running:
                    log.error(f"Meter WebSocket connection error: {e}")
                    log.error(traceback.format_exc())
                self.ws = None
                if self.running:
                    time.sleep(5)
    
    def start(self):
        """Start WebSocket client"""
        if self.running:
            log.warning("Meter client already running")
            return
        self.running = True
        self.thread = threading.Thread(target=self._run, daemon=True, name="MeterWebSocket")
        self.thread.start()
    
    def stop(self):
        """Stop WebSocket client"""
        self.running = False
        if self.ws:
            try:
                self.ws.close()
            except:
                pass
        if self.thread:
            self.thread.join(timeout=2)


class PipeWeaverWebSocketClient:
    """Full-featured WebSocket client for PipeWeaver with command support and patch handling"""
    
    def __init__(self, port=14565, patch_callback=None):
        if jsonpatch is None:
            raise ImportError("jsonpatch library is required. Install it with: pip install jsonpatch")

        self.port = port
        self.patch_callback = patch_callback
        self.ws = None
        self.running = False
        self.thread = None
        self.lock = threading.Lock()
        self.command_id = 0
        self.message_queue = {}
        self.status = None
        self.connected = False
    
    def _send_command(self, request_data, timeout=5.0):
        """Send a command and wait for response"""
        max_wait = 5.0
        wait_time = 0.0
        while not self.connected and wait_time < max_wait:
            time.sleep(0.1)
            wait_time += 0.1
        
        with self.lock:
            if not self.connected or not self.ws:
                log.error("WebSocket not connected")
                return None
            
            command_id = self.command_id
            self.command_id += 1
            
            ws_request = {
                "id": command_id,
                "data": request_data
            }
            
            response_queue = Queue()
            event = threading.Event()
            self.message_queue[command_id] = (response_queue, event)
        
        try:
            request_json = json.dumps(ws_request)

            with self.lock:
                ws = self.ws
                if not ws or not self.connected:
                    log.error("WebSocket not connected")
                    self.message_queue.pop(command_id, None)
                    return None

            try:
                ws.send(request_json)
            except Exception as e:
                log.error(f"Error sending command {command_id}: {e}")
                with self.lock:
                    self.message_queue.pop(command_id, None)
                return None
            
            if event.wait(timeout):
                try:
                    response = response_queue.get_nowait()
                    return response
                except Empty:
                    return None
            else:
                log.warning(f"Command {command_id} timed out")
                with self.lock:
                    self.message_queue.pop(command_id, None)
                return None
        except Exception as e:
            log.error(f"Error sending command: {e}")
            with self.lock:
                self.message_queue.pop(command_id, None)
            return None
    
    def _handle_message(self, message):
        """Handle incoming WebSocket message"""
        try:
            msg = json.loads(message)
            msg_id = msg.get("id")
            msg_data = msg.get("data")
            
            if msg_id is None or msg_data is None:
                log.warning(f"Invalid message format: {msg}")
                return
            
            max_u64 = 2**64 - 1
            if msg_id == max_u64 or (isinstance(msg_data, dict) and "Patch" in msg_data):
                if isinstance(msg_data, dict) and "Patch" in msg_data:
                    self._handle_patch(msg_data["Patch"])
                return

            with self.lock:
                if msg_id in self.message_queue:
                    response_queue, event = self.message_queue[msg_id]
                    if isinstance(msg_data, dict):
                        if "Status" in msg_data:
                            self.status = msg_data["Status"]
                            response_queue.put(("Status", self.status))
                        elif "Err" in msg_data:
                            response_queue.put(("Err", msg_data["Err"]))
                        elif "Pipewire" in msg_data:
                            response_queue.put(("Pipewire", msg_data["Pipewire"]))
                        else:
                            log.warning(f"Unknown response dict format: {msg_data}")
                            response_queue.put(("Unknown", msg_data))
                    elif msg_data == "Ok":
                        response_queue.put(("Ok", None))
                    else:
                        log.warning(f"Unknown response format: {type(msg_data)} - {msg_data}")
                        response_queue.put(("Unknown", msg_data))

                    event.set()
                    del self.message_queue[msg_id]
                else:
                    log.warning(f"Received response for unknown command ID: {msg_id}, data: {type(msg_data)}")
        except json.JSONDecodeError as e:
            log.error(f"Failed to parse message: {e}")
        except Exception as e:
            log.error(f"Error handling message: {e}")
            log.error(traceback.format_exc())
    
    def _handle_patch(self, patch):
        if not self.status:
            with self.lock:
                self.status = {}

        try:
            with self.lock:
                jsonpatch.apply_patch(self.status, patch, in_place=True)

            if self.patch_callback:
                self.patch_callback(self.status)
        except Exception as e:
            log.error(f"Error applying patch: {e}")
            log.error(traceback.format_exc())
    
    
    def _run(self):
        """Run WebSocket client in thread using websocket-client library"""
        while self.running:
            try:
                url = f"ws://localhost:{self.port}/api/websocket"

                self.ws = websocket.create_connection(url, timeout=5)
                self.connected = True
                
                self._request_initial_status_once()
                
                while self.running:
                    try:
                        self.ws.sock.settimeout(1.0)
                        message = self.ws.recv()
                        if message:
                            self._handle_message(message)
                    except websocket.WebSocketTimeoutException:
                        continue
                    except websocket.WebSocketConnectionClosedException:
                        log.warning("WebSocket connection closed")
                        break
                    except Exception as e:
                        log.error(f"Error receiving message: {e}")
                        log.error(traceback.format_exc())
                        break
                        
            except Exception as e:
                if self.running:
                    log.error(f"WebSocket connection error: {e}")
                    log.error(traceback.format_exc())
                self.connected = False
                if self.ws:
                    try:
                        self.ws.close()
                    except:
                        pass
                self.ws = None
                if self.running:
                    time.sleep(5)
    
    def start(self):
        """Start WebSocket client"""
        if self.running:
            log.warning("WebSocket client already running")
            return
        self.running = True
        self.thread = threading.Thread(target=self._run, daemon=True, name="PipeWeaverWebSocket")
        self.thread.start()
    
    def stop(self):
        """Stop WebSocket client"""
        self.running = False
        self.connected = False
        if self.ws:
            try:
                self.ws.close()
            except:
                pass
        if self.thread:
            self.thread.join(timeout=2)
    
    def _request_initial_status_once(self):
        """Request initial status once on connection - not polling, just one-time fetch"""
        def request_once():
            try:
                time.sleep(0.2)
                request = "GetStatus"
                response = self._send_command(request, timeout=10.0)
                if response and response[0] == "Status":
                    with self.lock:
                        self.status = response[1]  
                else:
                    log.warning(f"Initial status request failed: {response}")
            except Exception as e:
                log.warning(f"Error requesting initial status: {e} (patches may provide it)")
        
        threading.Thread(target=request_once, daemon=True, name="InitialStatusRequest").start()
    
    def _get_status(self):
        """Get status from cache - patches keep it updated, no polling"""
        with self.lock:
            return self.status
    
    def get_devices(self):
        """Get list of PipeWeaver devices"""
        status = self._get_status()
        if not status:
            return []
        
        devices = []
        try:
            profile = status.get("audio", {}).get("profile", {})
            devices_dict = profile.get("devices", {})
            
            sources = devices_dict.get("sources", {}).get("virtual_devices", [])
            for device in sources:
                devices.append({
                    "id": device["description"]["id"],
                    "name": device["description"]["name"],
                    "type": "source"
                })
            
            targets = devices_dict.get("targets", {}).get("virtual_devices", [])
            for device in targets:
                devices.append({
                    "id": device["description"]["id"],
                    "name": device["description"]["name"],
                    "type": "target"
                })
        except Exception as e:
            log.error(f"Failed to parse device list: {e}")
        
        return devices
    
    def _get_device_type(self, device_id):
        """Get device type (source/target) for a given device ID"""
        status = self._get_status()
        if not status:
            return None
        
        profile = status.get("audio", {}).get("profile", {})
        devices_dict = profile.get("devices", {})
        
        for device in devices_dict.get("sources", {}).get("virtual_devices", []):
            if device["description"]["id"] == device_id:
                return "source"
        
        for device in devices_dict.get("targets", {}).get("virtual_devices", []):
            if device["description"]["id"] == device_id:
                return "target"
        
        return None
    
    def mute_device(self, device_id, target=None):
        """Mute a device"""
        device_type = self._get_device_type(device_id)
        if not device_type:
            log.error(f"Device {device_id} not found")
            return False
        
        try:
            if device_type == "source":
                return self._mute_source_device(device_id, target)
            elif device_type == "target":
                return self._mute_target_device(device_id)
        except Exception as e:
            log.error(f"Error muting device: {e}")
            return False
    
    def _mute_source_device(self, device_id, target):
        """Mute a source device"""
        if target:
            mute_target = "TargetA" if target.upper() == "A" else "TargetB"
            command = {"AddSourceMuteTarget": [device_id, mute_target]}
            return self._send_pipewire_command(command)
        else:
            command_a = {"AddSourceMuteTarget": [device_id, "TargetA"]}
            command_b = {"AddSourceMuteTarget": [device_id, "TargetB"]}
            return (self._send_pipewire_command(command_a) and 
                    self._send_pipewire_command(command_b))
    
    def _mute_target_device(self, device_id):
        """Mute a target device"""
        command = {"SetTargetMuteState": [device_id, "Muted"]}
        return self._send_pipewire_command(command)
    
    def _send_pipewire_command(self, command):
        """Send a PipeWeaver command and return success status"""
        request = {"Pipewire": command}
        response = self._send_command(request)
        return response and response[0] == "Pipewire" and response[1] in ["Ok", {"Ok": None}]
    
    def unmute_device(self, device_id, target=None):
        """Unmute a device"""
        device_type = self._get_device_type(device_id)
        if not device_type:
            log.error(f"Device {device_id} not found")
            return False
        
        try:
            if device_type == "source":
                return self._unmute_source_device(device_id, target)
            elif device_type == "target":
                return self._unmute_target_device(device_id)
        except Exception as e:
            log.error(f"Error unmuting device: {e}")
            return False
    
    def _unmute_source_device(self, device_id, target):
        """Unmute a source device"""
        if target:
            mute_target = "TargetA" if target.upper() == "A" else "TargetB"
            command = {"DelSourceMuteTarget": [device_id, mute_target]}
            return self._send_pipewire_command(command)
        else:
            command_a = {"DelSourceMuteTarget": [device_id, "TargetA"]}
            command_b = {"DelSourceMuteTarget": [device_id, "TargetB"]}
            return (self._send_pipewire_command(command_a) and 
                    self._send_pipewire_command(command_b))
    
    def _unmute_target_device(self, device_id):
        """Unmute a target device"""
        command = {"SetTargetMuteState": [device_id, "Unmuted"]}
        return self._send_pipewire_command(command)
    
    def set_volume(self, device_id, volume, mix=None):
        """Set device volume (0-100)"""
        device_type = self._get_device_type(device_id)
        if not device_type:
            log.error(f"Device {device_id} not found")
            return False
        
        try:
            if device_type == "source":
                if not mix:
                    log.error("Mix parameter required for source devices")
                    return False
                mix_enum = mix.upper()
                command = {"SetSourceVolume": [device_id, mix_enum, volume]}
            elif device_type == "target":
                command = {"SetTargetVolume": [device_id, volume]}
            else:
                return False
            
            return self._send_pipewire_command(command)
        except Exception as e:
            log.error(f"Error setting volume: {e}")
            return False
    
    def set_volume_relative(self, device_id, delta, mix=None, current_volume=None):
        """Set device volume relative to current (delta can be positive or negative)"""
        if current_volume is None:
            return False
        
        new_volume = max(0, min(100, current_volume + delta))
        return self.set_volume(device_id, new_volume, mix)
    
    def set_volume_linked(self, device_id, linked):
        """Enable/disable volume linking for a source device"""
        command = {"SetSourceVolumeLinked": [device_id, linked]}
        request = {"Pipewire": command}
        response = self._send_command(request)
        
        if response and response[0] == "Pipewire":
            return response[1] in ["Ok", {"Ok": None}]
        return False
    
        
    def is_volume_linked(self, device_id):
        """Check if volumes are linked for a source device"""
        try:
            status = self._get_status()
            if status:
                profile = status.get("audio", {}).get("profile", {})
                devices = profile.get("devices", {})
                for device in devices.get("sources", {}).get("virtual_devices", []):
                    if device["description"]["id"] == device_id:
                        volumes = device.get("volumes", {})
                        volumes_linked = volumes.get("volumes_linked")
                        return volumes_linked is not None
        except Exception as e:
            log.error(f"Error checking link status: {e}")
        
        return False
