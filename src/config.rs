//! Bindings config (`bindings.toml`).
//!
//! The file is the source of truth: edit it, restart, and the device uses it.
//! These types derive `Serialize` + `Deserialize`, so a future GUI can load
//! the file with `load`, change `bindings`, and write it back with `save`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Where the config is read from and written to.
pub const DEFAULT_PATH: &str = "bindings.toml";

/// The whole file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Brightness applied when the program starts, 0..=10.
    #[serde(default = "default_brightness")]
    pub brightness: u8,

    /// One entry per physical control. Written as repeated `[[binding]]` tables.
    #[serde(default, rename = "binding")]
    pub bindings: Vec<Binding>,
}

/// One line of the table: which control, and what it does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    /// `press` or `rotate`.
    pub on: Input,

    /// Raw id from the device: knobs are 1 and 2, screen buttons are 7..=10.
    pub id: u8,

    /// How the key should look. Only used by screen keys (`on = "touch"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,

    /// The rest of the keys belong to the action, so the TOML stays flat.
    #[serde(flatten)]
    pub action: Action,
}

/// The face of one screen key: what is drawn in its 90x90 square.
///
/// Every field is optional, so a key can be just a colour, just an icon, just
/// text, or any mix. Anything left out falls back to a sensible default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Display {
    /// Text on the key. Falls back to the action's name when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Path to a PNG icon, relative to the folder `bindings.toml` is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Background colour, written `"#rrggbb"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,

    /// Colour of the label and icon, written `"#rrggbb"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ink: Option<String>,
}

/// Which kind of input a binding listens to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    /// A knob press or a screen button (see `Binding::id`).
    Press,
    /// A knob turn.
    Rotate,
    /// A touch on one of the screen keys, ids `0..=14`.
    Touch,
}

/// What to do when the input fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Play a haptic pattern.
    Vibrate { pattern: Pattern },

    /// Move the brightness by `step` per knob detent (negative turns it down).
    BrightnessStep {
        #[serde(default = "default_step")]
        step: i8,
    },

    /// Jump to an exact brightness.
    SetBrightness { level: u8 },

    /// Keep the input bound but do nothing (handy while editing the file).
    Disabled,
}

/// Haptic patterns, named after the firmware constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Short = 0x01,
    Medium = 0x0a,
    Long = 0x0f,
    Low = 0x31,
    ShortLow = 0x32,
    ShortLower = 0x33,
    Lower = 0x40,
    Lowest = 0x41,
    DescendSlow = 0x46,
    DescendMed = 0x47,
    DescendFast = 0x48,
    AscendSlow = 0x52,
    AscendMed = 0x53,
    AscendFast = 0x58,
    RevSlowest = 0x5e,
    RevSlow = 0x5f,
    RevMed = 0x60,
    RevFast = 0x61,
    RevFaster = 0x62,
    RevFastest = 0x63,
    RiseFall = 0x6a,
    Buzz = 0x70,
    VeryLong = 0x76,
    Rumble5 = 0x77,
    Rumble4 = 0x78,
    Rumble3 = 0x79,
    Rumble2 = 0x7a,
    Rumble1 = 0x7b,
}

impl Pattern {
    /// The byte the device expects for this pattern.
    pub fn byte(self) -> u8 {
        self as u8
    }
}

impl Action {
    /// Short description for the GUI, e.g. what to print inside a key square.
    pub fn label(&self) -> String {
        match self {
            Action::Vibrate { pattern } => format!("vibrate {pattern:?}"),
            // `{step:+}` prints the sign, so +1 and -2 are obvious.
            Action::BrightnessStep { step } => format!("brightness {step:+}"),
            Action::SetBrightness { level } => format!("brightness {level}/10"),
            Action::Disabled => "disabled".to_owned(),
        }
    }
}

fn default_brightness() -> u8 {
    5
}

fn default_step() -> i8 {
    1
}

impl Default for Config {
    /// What `bindings.toml` contains the first time the program runs.
    fn default() -> Self {
        Self {
            brightness: default_brightness(),
            bindings: vec![
                // Knob presses.
                press(
                    1,
                    Action::Vibrate {
                        pattern: Pattern::Short,
                    },
                ),
                press(
                    2,
                    Action::Vibrate {
                        pattern: Pattern::Medium,
                    },
                ),
                // The four buttons below the screen.
                press(
                    7,
                    Action::Vibrate {
                        pattern: Pattern::Short,
                    },
                ),
                press(
                    8,
                    Action::Vibrate {
                        pattern: Pattern::Medium,
                    },
                ),
                press(
                    9,
                    Action::Vibrate {
                        pattern: Pattern::Buzz,
                    },
                ),
                press(
                    10,
                    Action::Vibrate {
                        pattern: Pattern::Long,
                    },
                ),
                // Either knob adjusts the screen brightness.
                rotate(1, Action::BrightnessStep { step: 1 }),
                rotate(2, Action::BrightnessStep { step: 1 }),
            ],
        }
    }
}

fn press(id: u8, action: Action) -> Binding {
    Binding {
        on: Input::Press,
        id,
        display: None,
        action,
    }
}

fn rotate(id: u8, action: Action) -> Binding {
    Binding {
        on: Input::Rotate,
        id,
        display: None,
        action,
    }
}

impl Config {
    /// The binding for a specific control, if there is one.
    pub fn binding(&self, on: Input, id: u8) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|binding| binding.on == on && binding.id == id)
    }

    /// The action bound to a specific input, if there is one.
    pub fn action(&self, on: Input, id: u8) -> Option<&Action> {
        self.binding(on, id).map(|binding| &binding.action)
    }

    /// How a screen key should look, if the file says.
    pub fn display(&self, on: Input, id: u8) -> Option<&Display> {
        self.binding(on, id).and_then(|binding| binding.display.as_ref())
    }

    /// Write the config back to disk. A GUI will call this after editing.
    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let body = toml::to_string_pretty(self)?;
        fs::write(path, format!("{FILE_HEADER}{body}"))?;
        Ok(())
    }
}

/// Read the config, creating it from the defaults if it does not exist yet.
pub fn load_or_create(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
    if !path.exists() {
        let config = Config::default();
        config.save(path)?;
        println!("Created {}", path.display());
        return Ok(config);
    }

    let text = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&text)?;
    Ok(config)
}

/// Comment block at the top of the generated file. Not parsed.
const FILE_HEADER: &str = r##"# fiondeck-rust bindings
#
# Edit this file, then restart the program.
#
# on     = "press" (knob press or screen button), "rotate" (knob turn),
#          or "touch" (one screen key)
# id     = knobs: 1, 2          screen buttons: 7, 8, 9, 10
#          screen keys: 0..14   (5 across, 3 down, counted left to right)
# action = "vibrate" | "brightness_step" | "set_brightness" | "disabled"
#
# vibrate:          pattern = "short" | "medium" | "long" | "buzz" | ...
# brightness_step:  step    = 1     (per knob detent, negative = down)
# set_brightness:   level   = 0..10
# disabled:         no extra keys
#
# A screen key can also say what it looks like:
#
#   display = { label = "Mic", icon = "icons/mic-on.png",
#               colour = "#2f6f4f", ink = "#ffffff" }
#
# display is optional and so is every field in it. `icon` is a PNG path
# relative to this file; a one-colour glyph works best, because it is drawn in
# `ink` whatever colour the file itself is. Without a label the action's name
# is shown instead.

"##;

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
brightness = 4

[[binding]]
on = "press"
id = 1
action = "vibrate"
pattern = "short"

[[binding]]
on = "rotate"
id = 1
action = "brightness_step"
step = 2

[[binding]]
on = "press"
id = 10
action = "set_brightness"
level = 10

[[binding]]
on = "press"
id = 9
action = "disabled"
"#;

    #[test]
    fn parses_hand_written_file() {
        let config: Config = toml::from_str(EXAMPLE).unwrap();

        assert_eq!(config.brightness, 4);
        assert_eq!(
            config.action(Input::Press, 1),
            Some(&Action::Vibrate {
                pattern: Pattern::Short
            })
        );
        assert_eq!(
            config.action(Input::Rotate, 1),
            Some(&Action::BrightnessStep { step: 2 })
        );
        assert_eq!(
            config.action(Input::Press, 10),
            Some(&Action::SetBrightness { level: 10 })
        );
        assert_eq!(config.action(Input::Press, 9), Some(&Action::Disabled));
        assert_eq!(config.action(Input::Press, 2), None);
    }

    #[test]
    fn parses_touch_binding() {
        let text = r#"
[[binding]]
on = "touch"
id = 3
action = "vibrate"
pattern = "buzz"
"#;
        let config: Config = toml::from_str(text).unwrap();
        assert_eq!(
            config.action(Input::Touch, 3),
            Some(&Action::Vibrate {
                pattern: Pattern::Buzz
            })
        );
        assert_eq!(config.action(Input::Touch, 4), None);
    }

    #[test]
    fn labels_read_like_the_gui_shows_them() {
        assert_eq!(
            Action::BrightnessStep { step: 2 }.label(),
            "brightness +2".to_owned()
        );
        assert_eq!(
            Action::SetBrightness { level: 7 }.label(),
            "brightness 7/10".to_owned()
        );
        assert_eq!(Action::Disabled.label(), "disabled".to_owned());
    }

    #[test]
    fn defaults_round_trip_through_toml() {
        let config = Config::default();
        let text = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn parses_a_display_block() {
        let text = r##"
[[binding]]
on = "touch"
id = 5
action = "vibrate"
pattern = "short"
display = { label = "Mic", icon = "icons/mic-on.png", colour = "#2f6f4f", ink = "#ffffff" }
"##;

        let config: Config = toml::from_str(text).unwrap();
        let display = config.binding(Input::Touch, 5).unwrap().display.as_ref().unwrap();

        assert_eq!(display.label.as_deref(), Some("Mic"));
        assert_eq!(display.icon.as_deref(), Some("icons/mic-on.png"));
        assert_eq!(display.colour.as_deref(), Some("#2f6f4f"));
        assert_eq!(display.ink.as_deref(), Some("#ffffff"));

        // The action still parses next to the extra table.
        assert_eq!(
            config.action(Input::Touch, 5),
            Some(&Action::Vibrate {
                pattern: Pattern::Short
            })
        );
    }

    #[test]
    fn a_display_block_can_be_partial() {
        let text = r##"
[[binding]]
on = "touch"
id = 0
action = "disabled"
display = { colour = "#101010" }
"##;

        let config: Config = toml::from_str(text).unwrap();
        let display = config.display(Input::Touch, 0).unwrap();

        assert_eq!(display.colour.as_deref(), Some("#101010"));
        assert_eq!(display.label, None);
        assert_eq!(display.icon, None);
    }

    #[test]
    fn bindings_without_a_display_block_have_none() {
        let config: Config = toml::from_str(EXAMPLE).unwrap();

        assert_eq!(config.display(Input::Press, 1), None);
    }

    #[test]
    fn step_defaults_to_one() {
        let text = r#"
[[binding]]
on = "rotate"
id = 2
action = "brightness_step"
"#;
        let config: Config = toml::from_str(text).unwrap();
        assert_eq!(config.brightness, 5);
        assert_eq!(
            config.action(Input::Rotate, 2),
            Some(&Action::BrightnessStep { step: 1 })
        );
    }
}
