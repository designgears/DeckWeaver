<img width="629" height="102" alt="image" src="https://github.com/user-attachments/assets/2dfc5dd2-052e-4d4c-a382-b2e653eee3e3" />

# DeckWeaver

A Stream Deck plugin for controlling PipeWeaver virtual audio devices. Works with **StreamController** and **OpenDeck**, sharing one Rust core for PipeWeaver IPC and rendering.

## What is PipeWeaver?

PipeWeaver is a virtual audio routing system that allows you to create and manage virtual audio devices. This plugin gives you physical control over those virtual devices directly from your Stream Deck, enabling real-time audio management with visual feedback.

## Features

### Core Functionality
- **Volume Control**: Adjust audio levels with precise steps (5-20% per step, configurable)
- **Mute Toggle**: Quickly mute/unmute audio devices with visual feedback
- **Device Selection**: Control any available PipeWeaver virtual device (source or target)
- **Hardware Output Switching**: Reassign a hardware output target to a selected physical output device with a dedicated button
- **Real-time Feedback**: Visual indicators show current audio levels, mute status, and device state
- **Service Monitoring**: Automatic detection of PipeWeaver daemon availability with visual status indicators

### Stream Deck Integration
- **Knob Support**: Full support for Stream Deck+ and Studio dials
  - Turn clockwise/counter-clockwise for volume up/down
  - Press to toggle mute
- **Visual Feedback**: Dynamic icons and volume bars show device status and audio levels
- **Real-time Metering**: Visual audio level meters for source and target devices

### Configuration
- **Multi-language Support**: English (en_US), Spanish (es_ES), Chinese (zh_CN), French (fr_FR), German (de_DE)
- **Custom Icons**: Use StreamController icon packs or custom SVG/PNG files
- **Adjustable Steps**: Configure volume step size (5-20%) per your preference
- **Meter Controls**: Enable/disable audio level meters, customize meter color, and invert meter color
- **Volume Bar Color**: Customize volume bar color or use device color
- **Persistent Settings**: Device selections and configurations are saved automatically

## Building (developers)

The project is a Cargo workspace with a shared core and two host targets:

| Crate | Output | Host |
|-------|--------|------|
| `deckweaver-core` | Rust library | PipeWeaver IPC + rendering |
| `deckweaver-py` | `deckweaver/_core.abi3.so` | StreamController (Python/PyO3) |
| `deckweaver-opendeck` | `opendeck/.../bin/deckweaver` | OpenDeck (OpenAction binary) |

```bash
# Build both targets (default)
./build.sh release

# StreamController only (--sc and -sc also work)
./build.sh release --streamcontroller

# OpenDeck only (--od and -od also work)
./build.sh release --opendeck

# Build and install both
./build.sh release --install

# Install OpenDeck plugin bundle only
./build.sh release --opendeck --install
```

`--install`/`-i` installs whichever targets the build selected, so pair it with
`--streamcontroller` or `--opendeck` to install just one.

The Python extension uses PyO3's abi3 stable ABI and works on **any Python 3.11+**.

- **StreamController:** `./build.sh release` or `pip install .` (maturin)
- **OpenDeck:** copy or symlink `opendeck/com.designgears.deckweaver.sdPlugin` into your OpenDeck plugins directory, or use `--opendeck --install`

**Version:** Set once in the workspace `Cargo.toml` (`[workspace.package] version`). The build script syncs it to `pyproject.toml`, `manifest.json`, and the OpenDeck manifest.

## Requirements

- **PipeWeaver**: Daemon running on `localhost:14565` (Linux)
- **One host app:**
  - **StreamController** 1.5.0-beta.12 or later, or
  - **OpenDeck** with Stream Deck-compatible plugin support
- **Stream Deck device** (Stream Deck+, Studio, etc. recommended for dial actions)

## Installation

### StreamController

1. Install the plugin through StreamController's plugin manager (or `./build.sh release --install`)
2. Ensure PipeWeaver is running on `localhost:14565`
3. Add a DeckWeaver action and configure devices in the plugin UI

### OpenDeck

1. Run `./build.sh release --opendeck` (add `--install` to deploy it automatically)
2. Place `opendeck/com.designgears.deckweaver.sdPlugin` in your OpenDeck plugins folder
3. Restart OpenDeck / reload plugins
4. Add a DeckWeaver action and configure it in the property inspector

## Usage

### Basic Controls
- **Turn dial clockwise**: Increase volume by configured step amount
- **Turn dial counter-clockwise**: Decrease volume by configured step amount
- **Press dial**: Toggle mute/unmute

### Configuration Options
- **Device Selection**: Choose audio device from available PipeWeaver devices (with refresh button)
- **Custom Icon**: Select custom icons from StreamController icon packs or custom SVG/PNG files
- **Volume Step**: Adjust volume step size (5-20% increments)
- **Meters Enabled**: Toggle audio level meter display on/off
- **Meter Color**: Customize meter color or invert volume bar color for meters
- **Volume Bar Color**: Override volume bar color or use device color automatically
- **Language**: Set interface language or use OS default (in plugin settings)
- **Output Device Switch Button**: Pick a hardware output target and the physical device that should be attached when pressed

## Device Types

- **Source Devices**: Input virtual devices with volume and mute control
- **Target Devices**: Output virtual devices with direct volume and mute control

## Support

For issues related to:
- **Plugin functionality**: Create an issue on GitHub
- **PipeWeaver daemon**: Refer to PipeWeaver documentation
- **StreamController**: Check StreamController documentation and support channels
