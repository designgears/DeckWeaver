<img width="629" height="102" alt="image" src="https://github.com/user-attachments/assets/2dfc5dd2-052e-4d4c-a382-b2e653eee3e3" />

# DeckWeaver

A Stream Deck plugin for controlling PipeWeaver virtual audio devices. Works with **StreamController** and **OpenDeck**, sharing one Rust core for PipeWeaver IPC and rendering.

## What is PipeWeaver?

PipeWeaver is a virtual audio routing system that allows you to create and manage virtual audio devices. This plugin gives you physical control over those virtual devices directly from your Stream Deck, enabling real-time audio management with visual feedback.

## Features

### Core Functionality
- **Volume Control**: Adjust audio levels with precise steps (5-20% per step, configurable)
- **Mute Toggle**: Quickly mute/unmute audio devices (sources and targets) with visual feedback
- **Press and Hold**: Volume and slider keys keep stepping while held, at a per-key adjustable speed ("Hold repeat speed", 50–1000 ms between steps)
- **Route Toggle**: Turn the route from a source into a target on or off with one key (e.g. hear your microphone in your headphones only when you want to)
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

Built files (`deckweaver/_core.abi3.so`, the OpenDeck binary, `deckweaver.streamDeckPlugin`) are not committed; `build.sh` writes them into the tree for local use and they are gitignored.

## Releasing

Releases are built by GitHub Actions ([`.github/workflows/build.yml`](.github/workflows/build.yml)). Push a tag and CI does the rest:

```bash
git tag v0.8.0
git push origin v0.8.0
```

CI runs the tests, builds both plugins with `release.sh`, and publishes the GitHub release `v0.8.0` with:

| File | For |
|------|-----|
| `deckweaver-streamcontroller-<version>.zip` | StreamController |
| `deckweaver-opendeck-<version>.streamDeckPlugin` | OpenDeck |
| `SHA256SUMS` | checksums |

Versions live only in git tags: the repo's manifests say `0.0.0`, and every build stamps the real version into what it produces, so there is no version-bump commit. A release build takes it from its tag; a local `./build.sh` takes it from `git describe` (`scripts/version.sh`), e.g. `0.8.1-8-ge50c4f1` for 8 commits after v0.8.1, with `-dirty` for uncommitted changes. A tag with a pre-release part (`v0.8.0-beta.1`) publishes a pre-release. Pushes to `main` and pull requests build and test the same way, keeping the files as a workflow artifact without releasing; **Actions → Build → Run workflow** can also build (and optionally release) a given version.

To build the release files locally: `./release.sh 0.8.0` (needs a clean tree; output in `out/0.8.0/`).

## Requirements

- **PipeWeaver**: Daemon running on `localhost:14565` (Linux)
- **One host app:**
  - **StreamController** 1.5.0-beta.12 or later, or
  - **OpenDeck** with Stream Deck-compatible plugin support
- **Stream Deck device** (Stream Deck+, Studio, etc. recommended for dial actions)

## Installation

### StreamController

1. Download `deckweaver-streamcontroller-<version>.zip` from the [latest release](https://github.com/designgears/DeckWeaver/releases/latest) and unzip it into StreamController's plugins folder (`~/.var/app/com.core447.StreamController/data/plugins/` for the Flatpak), or build and install from source with `./build.sh release --streamcontroller --install`
2. Ensure PipeWeaver is running on `localhost:14565`
3. Add a DeckWeaver action and configure devices in the plugin UI

### OpenDeck

1. Download `deckweaver-opendeck-<version>.streamDeckPlugin` from the [latest release](https://github.com/designgears/DeckWeaver/releases/latest) and install it from OpenDeck's plugin manager, or build and install from source with `./build.sh release --opendeck --install`
2. Restart OpenDeck / reload plugins
3. Add a DeckWeaver action and configure it in the property inspector

## Usage

### Basic Controls
- **Turn dial clockwise**: Increase volume by configured step amount
- **Turn dial counter-clockwise**: Decrease volume by configured step amount
- **Press dial**: Toggle mute/unmute
- **Volume key**: Positive step raises the volume, negative lowers it, `0` makes it a mute toggle. Hold the key to keep stepping.
- **Route Toggle key**: Turns the selected source → target route on or off; the key shows a slash while the route is off

### Configuration Options
- **Device Selection**: Choose audio device from available PipeWeaver devices (with refresh button)
- **Custom Icon**: Select custom icons from StreamController icon packs or custom SVG/PNG files
- **Volume Step**: Adjust volume step size (5-20% increments)
- **Meters Enabled**: Toggle audio level meter display on/off
- **Meter Color**: Customize meter color or invert volume bar color for meters
- **Volume Bar Color**: Override volume bar color or use device color automatically
- **Language**: Set interface language or use OS default (in plugin settings)
- **Output Device Switch Button**: Pick a hardware output target and the physical device that should be attached when pressed
- **Route Toggle Button**: Pick a source and a target; each press routes or un-routes that source into that target

## Device Types

- **Source Devices**: Input virtual devices with volume and mute control
- **Target Devices**: Output virtual devices with direct volume and mute control

## Support

For issues related to:
- **Plugin functionality**: Create an issue on GitHub
- **PipeWeaver daemon**: Refer to PipeWeaver documentation
- **StreamController**: Check StreamController documentation and support channels
