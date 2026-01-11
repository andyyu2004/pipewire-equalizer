# pipewire-equalizer

TUI equalizer for PipeWire.

## Demo

[![asciicast](https://asciinema.org/a/767183.svg)](https://asciinema.org/a/767183)

## Requirements

PipeWire v1.0+ must be installed and running on your system.

## Installation

```bash
cargo install --path pw-eq
```


## Usage

```bash
pw-eq
```

Create default config for modification. The configuration is in the [spa-json](https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/conf_file.html) format, a superset of JSON.
```bash
pw-eq config init
```

```bash
pw-eq tui # starts TUI equalizer with default filters
```

Load from a file:
```bash
pw-eq tui --file <PATH> # only .apo format is currently supported
```

Load a preset:
```bash
pw-eq tui --preset flat<n>
```

Save configuration to a file:
```bash
# Within TUI command line
:w <PATH>
```

