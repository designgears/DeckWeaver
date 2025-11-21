# DeckWeaver

A Stream Deck plugin for controlling PipeWeaver virtual audio devices. Provides hardware control for volume, mute, and audio routing through your Stream Deck device.

## What is PipeWeaver?

PipeWeaver is a virtual audio routing system that allows you to create and manage virtual audio devices. This plugin gives you physical control over those virtual devices directly from your Stream Deck.

## Features

### Core Functionality
- **Volume Control**: Adjust audio levels with precise steps (1-20% per step)
- **Mute Toggle**: Quickly mute/unmute audio devices
- **Device Selection**: Control any available PipeWeaver virtual device
- **Real-time Feedback**: Visual indicators show current audio levels and mute status

### Stream Deck Integration
- **Knob Support**: Full support for Stream Deck+ and Studio dials
  - Turn clockwise/counter-clockwise for volume up/down
  - Press to toggle mute
  - Touchscreen menu for advanced controls
- **Touchscreen Interface**: Interactive menu system on compatible devices
- **Visual Feedback**: Dynamic icons show device status and audio levels

### Audio Routing Control
- **Mix Selection**: Control Mix A and Mix B independently for source devices
- **Target Routing**: Choose which outputs to mute to
- **Volume Linking**: Link/unlink Mix A and B volumes for synchronized control
- **Smart Bus Cycling**: Intelligent bus selection based on current state

### Configuration
- **Multi-language Support**: English, Spanish, Chinese, French, German
- **Custom Icons**: Use StreamController icon packs
- **Adjustable Steps**: Configure volume step size per your preference
- **Persistent Settings**: Device selections and configurations are saved

## Requirements

- StreamController 1.5.0-beta.12 or later
- PipeWeaver daemon running
- Stream Deck device (recommended: Stream Deck+ or Studio for full functionality)

## Installation

1. Install the plugin through StreamController
2. Ensure PipeWeaver daemon is running
3. Configure your preferred devices and settings

## Usage

### Basic Controls
- **Turn dial**: Adjust volume up/down
- **Press dial**: Toggle mute
- **Long press**: Open advanced menu (on touchscreen devices)

### Advanced Menu (Touchscreen)
- **Link/Unlink**: Synchronize Mix A and B volume control
- **Bus Selection**: Toggle Mix A, Mix B, or both
- **Quick Actions**: Access common routing options

### Configuration Options
- Select target audio device from available PipeWeaver devices
- Choose which mixes to control (A, B, or both)
- Set mute targets for source devices
- Adjust volume step size
- Customize icon appearance

## Technical Details

### Connection
- WebSocket client connects to PipeWeaver
- Real-time status updates via JSON patch stream
- Automatic reconnection on connection loss

### Device Types
- **Source Devices**: Input virtual devices with Mix A/B routing
- **Target Devices**: Output virtual devices with direct volume control

### Meter Data
- Real-time audio level monitoring
- Visual feedback on Stream Deck display
- Separate metering for Mix A, Mix B, and target devices

## License

See LICENSE file for details.

## Contributing

GitHub: https://github.com/designgears/DeckWeaver

## Support

For issues related to:
- **Plugin functionality**: Create an issue
- **PipeWeaver daemon**: Refer to PipeWeaver documentation
- **StreamController**: Check StreamController documentation and support channels
