# pipewire-equalizer

TUI equalizer for PipeWire.

## Demo

[![asciicast](https://asciinema.org/a/767183.svg)](https://asciinema.org/a/767183)

## Requirements

PipeWire v1.0+ must be installed and running on your system.

## Installation

### Depedencies
- pipewire development headers
- pipewire-utils (pw-dump)
- wireplumber (wpctl)

```bash
cargo install --git https://github.com/andyyu2004/pipewire-equalizer
# Or if cloned locally
cargo install --path pw-eq
```


## Usage

The intended workflow is to tweak the equalizer interactively using the TUI, then save the configuration to a file for PipeWire to load on startup.

Create default config for modification.
The configuration is in the [spa-json](https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/conf_file.html) format, a superset of JSON.
This allows modification of keybinds and the theme.

```bash
pw-eq config init
```

The default keybinding scheme uses an `esdf` (shifted `wasd`) layout for filter manipulation.
- `e`/`d` - increase/decrease gain (vertical axis)
- `s`/`f` - decrease/increase frequency (horizontal axis)
- `w`/`r` - decrease/increase Q factor (bandwidth control, positioned above)
- `tab/s-tab`- toggle filter type. Low-pass, high-pass, band-pass, notch, peak, low-shelf, high-shelf are supported.
- `j`/`k` - move selection down/up


```bash
# Starts TUI equalizer with default filters available.
# See the config file or press ? to see all available keybinds.
pw-eq
```

Load from a file:
```bash
pw-eq tui --file <PATH> # .apo,.txt or pipewire libpipewire-module-filter-chain .conf format supported.
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

