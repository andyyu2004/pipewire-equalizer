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

Intended workflow is to

```bash
pw-eq
```

Create default config for modification. The configuration is in the [spa-json](https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/conf_file.html) format, a superset of JSON.
Can modify keybinds and the theme.
```bash
pw-eq config init
```

```bash
# Starts TUI equalizer with default filters available.
# See the config file or press ? to see available keybinds.
pw-eq tui
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
# Within the TUI command line:
:w <PATH>.{conf,apo}
# If a relative path is provided:
# .conf format is saved to `$XDG_CONFIG_HOME/pipewire/pipewire.conf.d/<PATH>`. Pipewire must be restarted to pick up new config.
# .apo format is saved to `$(pwd)/<PATH>`.
```

