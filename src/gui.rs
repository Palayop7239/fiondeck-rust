//! Device preview, modelled on the layout view in the official Loupedeck app:
//! the panel as it sits on your desk, with the bound action written in each
//! square.
//!
//! Run with:
//!     cargo run -- --gui
//!
//! It reads `bindings.toml` and draws it. Nothing is sent to the device yet.
//!
//! Every iced app is three functions, at the bottom of this file:
//!
//!   boot   -> build the starting state (here: the config)
//!   update -> change the state in response to a Message
//!   view   -> describe the screen from the state (never changes anything)
//!
//! The loop is: widget sends Message -> update runs -> view runs again.

use std::path::Path;

use iced::widget::{column, container, text, Column, Row};
use iced::{border, Alignment, Color, Element, Length, Theme};

use crate::config::{self, Action, Config, Input};
use crate::surface;

/// Size of one drawn key or button, in window pixels.
/// The real keys are 96x90; this is the same shape, scaled down to fit.
const CELL_WIDTH: f32 = 86.0;
const CELL_HEIGHT: f32 = 78.0;

/// Knobs get a wider box so both bindings fit on one line each.
const KNOB_WIDTH: f32 = 150.0;

const GAP: f32 = 6.0;

/// What the window knows. Add fields here as the app grows.
struct App {
    config: Config,
}

/// One square: a small caption, then the action bound to it.
///
/// `bound` only changes the colours, so an empty key is obvious at a glance.
fn square<'a>(caption: String, body: String, bound: bool, width: f32) -> Element<'a, ()> {
    container(
        column![text(caption).size(10), text(body).size(12)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(width))
    .height(Length::Fixed(CELL_HEIGHT))
    .padding(4)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_theme| {
        if bound {
            container::Style {
                background: Some(Color::from_rgb(0.16, 0.27, 0.38).into()),
                border: border::rounded(8)
                    .width(1)
                    .color(Color::from_rgb(0.33, 0.56, 0.78)),
                ..container::Style::default()
            }
        } else {
            container::Style {
                background: Some(Color::from_rgb(0.12, 0.12, 0.14).into()),
                border: border::rounded(8)
                    .width(1)
                    .color(Color::from_rgb(0.22, 0.22, 0.26)),
                ..container::Style::default()
            }
        }
    })
    .into()
}

/// A physical button: bound to a `press` action.
fn button_square<'a>(config: &'a Config, id: u8) -> Element<'a, ()> {
    let action = config.action(Input::Press, id);
    square(
        format!("button {id}"),
        action.map(Action::label).unwrap_or_else(|| "unbound".into()),
        action.is_some(),
        CELL_WIDTH,
    )
}

/// A knob: one `rotate` action and one `press` action.
fn knob_square<'a>(config: &'a Config, id: u8) -> Element<'a, ()> {
    let rotate = config.action(Input::Rotate, id);
    let press = config.action(Input::Press, id);

    let body = format!(
        "turn  {}\npress {}",
        rotate.map(Action::label).unwrap_or_else(|| "-".into()),
        press.map(Action::label).unwrap_or_else(|| "-".into()),
    );

    square(
        format!("knob {id}"),
        body,
        rotate.is_some() || press.is_some(),
        KNOB_WIDTH,
    )
}

/// The 5x3 screen, one square per key.
fn screen<'a>(config: &'a Config) -> Column<'a, ()> {
    let mut grid = Column::new().spacing(GAP);

    for row_index in 0..surface::ROWS {
        let mut line = Row::new().spacing(GAP);

        for column_index in 0..surface::COLUMNS {
            let id = row_index * surface::COLUMNS + column_index;
            let action = config.action(Input::Touch, id);

            line = line.push(square(
                format!("key {id}"),
                action.map(Action::label).unwrap_or_else(|| "unbound".into()),
                action.is_some(),
                CELL_WIDTH,
            ));
        }

        grid = grid.push(line);
    }

    grid
}

// --- the three iced functions -------------------------------------------

/// The starting state: whatever is in bindings.toml right now.
fn boot() -> App {
    let config = config::load_or_create(Path::new(config::DEFAULT_PATH)).unwrap_or_else(|error| {
        eprintln!("could not load {}: {error}", config::DEFAULT_PATH);
        Config::default()
    });

    App { config }
}

/// Nothing is interactive yet, so `Message` is the empty type `()`.
fn update(_app: &mut App, _message: ()) {}

/// Describe the whole screen from the state.
fn view(app: &App) -> Element<'_, ()> {
    // Two buttons left of the screen, two right. Which id is which corner is
    // cosmetic here -- ids 7..=10 are what the firmware reports.
    let left = Column::new()
        .spacing(GAP)
        .push(button_square(&app.config, surface::BUTTON_IDS[0]))
        .push(button_square(&app.config, surface::BUTTON_IDS[1]));

    let right = Column::new()
        .spacing(GAP)
        .push(button_square(&app.config, surface::BUTTON_IDS[2]))
        .push(button_square(&app.config, surface::BUTTON_IDS[3]));

    let surface_row = Row::new()
        .spacing(GAP * 2.0)
        .align_y(Alignment::Center)
        .push(left)
        .push(screen(&app.config))
        .push(right);

    // The knobs sit below the panel on the Live S.
    let knobs = Row::new()
        .spacing(GAP * 2.0)
        .push(knob_square(&app.config, surface::KNOB_IDS[0]))
        .push(knob_square(&app.config, surface::KNOB_IDS[1]));

    let device = column![surface_row, knobs]
        .spacing(GAP * 2.0)
        .align_x(Alignment::Center);

    let panel = container(device).padding(18).style(|_theme| container::Style {
        background: Some(Color::from_rgb(0.07, 0.07, 0.09).into()),
        border: border::rounded(14)
            .width(1)
            .color(Color::from_rgb(0.20, 0.20, 0.24)),
        ..container::Style::default()
    });

    column![
        text("FionDeck").size(26),
        text(format!(
            "brightness {}/10   -   {} bindings   -   {}",
            app.config.brightness,
            app.config.bindings.len(),
            config::DEFAULT_PATH
        ))
        .size(12),
        panel,
        text("edit bindings.toml, then restart to see it here").size(11),
    ]
    .spacing(14)
    .align_x(Alignment::Center)
    .into()
}

/// Start the window. Blocks until the window is closed.
pub fn run() -> iced::Result {
    iced::application(boot, update, view)
        .title("FionDeck")
        .theme(Theme::Dark)
        .window_size((780.0, 560.0))
        .centered()
        .run()
}
