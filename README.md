# fiondeck-rust

Talk to a Loupedeck Live S (or Razer Stream Controller) over its serial port:
read knobs, buttons and touch, and send brightness and haptics back.

The wire format was reverse engineered from
[foxxyz/loupedeck](https://github.com/foxxyz/loupedeck) (MIT).

## Run

```sh
cargo run
```

Make sure the official Loupedeck software is not running — it holds the port.

The device is discovered by USB vendor id (`0x2EC2` / `0x1532`), so no COM port
is hardcoded. On startup the program loads `bindings.toml`, applies the
configured brightness, then prints every input event.

## Config: `bindings.toml`

Created automatically on first run. Edit it, then restart. One `[[binding]]`
table per input:

```toml
brightness = 5          # starting screen brightness, 0..=10

[[binding]]
on = "press"            # "press" = knob press or screen button
id = 1                  # knob ids: 1 (top), 2 (bottom)
action = "vibrate"
pattern = "short"

[[binding]]
on = "rotate"           # "rotate" = knob turn
id = 1
action = "brightness_step"
step = 1                # per detent, negative reverses

[[binding]]
on = "press"
id = 10                 # screen buttons are ids 7, 8, 9, 10
action = "set_brightness"
level = 10

[[binding]]
on = "press"
id = 9
action = "disabled"     # bound, but does nothing
```

### Actions

| `action` | Extra keys | Effect |
|---|---|---|
| `vibrate` | `pattern = "..."` | Play a haptic pattern |
| `brightness_step` | `step = 1` | Change brightness per knob detent |
| `set_brightness` | `level = 0..10` | Jump to an exact brightness |
| `disabled` | — | Ignore the input |

### Patterns

`short`, `medium`, `long`, `low`, `short_low`, `short_lower`, `lower`,
`lowest`, `descend_slow`, `descend_med`, `descend_fast`, `ascend_slow`,
`ascend_med`, `ascend_fast`, `rev_slowest`, `rev_slow`, `rev_med`, `rev_fast`,
`rev_faster`, `rev_fastest`, `rise_fall`, `buzz`, `very_long`, `rumble1` …
`rumble5`.

### Notes

- Unbound ids print `(no binding for press id N)` — useful when finding out
  which physical button is which.
- `Config` in `src/config.rs` is a plain serde struct with `load_or_create`
  and `save`, so a GUI can edit `config.bindings` and write it back in the same
  file format.

## Layout

| File | Purpose |
|---|---|
| `src/main.rs` | Event loop, logging, running configured actions |
| `src/config.rs` | `bindings.toml` types, defaults, load/save |
| `src/serial_worker.rs` | Thread that owns the serial port (reads + writes) |
| `src/protocol.rs` | Packet framing and outgoing commands |
| `src/parser.rs` | Incoming packet → `DeviceEvent` |
| `src/messages.rs` | Channel/event types and labels |
| `src/discovery.rs` | Find the port by USB vendor id |

## Shutdown

`Ctrl+C` clears the stop flag; the worker's read loop exits and drops the port,
so the device can be used again immediately.
