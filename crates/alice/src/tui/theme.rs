//! Colors and the two small pieces of motion the TUI uses: a time-driven
//! spinner for in-flight work and an ease-out fade for arriving status text.
use ratatui::style::{Color, Modifier, Style};
use std::time::Duration;

/// How long a freshly set status takes to settle from its highlight color.
pub const STATUS_FADE: Duration = Duration::from_millis(600);
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_FRAME_MS: u128 = 80;

pub struct Theme {
    /// `NO_COLOR` is set: every color is the terminal default and emphasis
    /// falls back to modifiers.
    pub plain: bool,
    pub accent: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub selection: Color,
    pub chip: Color,
    pub chip_text: Color,
    pub star: Color,
    pub flash: Color,
    pub status: Color,
    pub error_flash: Color,
    pub error: Color,
}

impl Theme {
    pub fn from_env() -> Self {
        if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
            return Self {
                plain: true,
                accent: Color::Reset,
                text: Color::Reset,
                muted: Color::Reset,
                faint: Color::Reset,
                selection: Color::Reset,
                chip: Color::Reset,
                chip_text: Color::Reset,
                star: Color::Reset,
                flash: Color::Reset,
                status: Color::Reset,
                error_flash: Color::Reset,
                error: Color::Reset,
            };
        }
        Self {
            plain: false,
            accent: Color::Rgb(180, 155, 225),
            text: Color::Reset,
            muted: Color::Rgb(140, 140, 157),
            faint: Color::Rgb(78, 76, 96),
            selection: Color::Rgb(46, 38, 64),
            chip: Color::Rgb(62, 52, 86),
            chip_text: Color::Rgb(226, 214, 255),
            star: Color::Rgb(240, 198, 116),
            flash: Color::Rgb(226, 212, 255),
            status: Color::Rgb(172, 170, 190),
            error_flash: Color::Rgb(255, 168, 150),
            error: Color::Rgb(240, 120, 104),
        }
    }

    pub fn fg(&self, color: Color) -> Style {
        Style::new().fg(color)
    }

    pub fn selected(&self) -> Style {
        if self.plain {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().bg(self.selection)
        }
    }

    /// The highlighted folder tab and key hint chips.
    pub fn chip(&self) -> Style {
        if self.plain {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().bg(self.chip).fg(self.chip_text)
        }
    }

    pub fn active_tab(&self) -> Style {
        if self.plain {
            Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::new()
                .bg(self.accent)
                .fg(Color::Rgb(24, 20, 34))
                .add_modifier(Modifier::BOLD)
        }
    }

    /// Color of a status message `age` after it was set: starts at `from`
    /// and settles on `to` with an ease-out curve, so it lands quickly.
    pub fn fade(&self, from: Color, to: Color, age: Duration) -> Color {
        if age >= STATUS_FADE {
            return to;
        }
        let t = ease_out(age.as_secs_f32() / STATUS_FADE.as_secs_f32());
        match (from, to) {
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
                Color::Rgb(mix(r1, r2, t), mix(g1, g2, t), mix(b1, b2, t))
            }
            _ => to,
        }
    }
}

/// Cubic ease-out: fast start, gentle landing.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn mix(from: u8, to: u8, t: f32) -> u8 {
    (f32::from(from) + (f32::from(to) - f32::from(from)) * t).round() as u8
}

/// Braille spinner frame for the time since the TUI started, so the speed is
/// independent of how often frames are drawn.
pub fn spinner(elapsed: Duration) -> &'static str {
    SPINNER[(elapsed.as_millis() / SPINNER_FRAME_MS) as usize % SPINNER.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_eases_out_and_settles() {
        let theme = Theme::from_env();
        let from = Color::Rgb(200, 200, 200);
        let to = Color::Rgb(100, 100, 100);
        assert_eq!(theme.fade(from, to, Duration::ZERO), from);
        assert_eq!(theme.fade(from, to, STATUS_FADE), to);
        // Ease-out covers most of the distance in the first half.
        let Color::Rgb(half, _, _) = theme.fade(from, to, STATUS_FADE / 2) else {
            panic!("expected rgb");
        };
        assert!(half < 120);
    }

    #[test]
    fn spinner_advances_with_time() {
        assert_eq!(spinner(Duration::ZERO), "⠋");
        assert_eq!(spinner(Duration::from_millis(80)), "⠙");
        assert_eq!(spinner(Duration::from_millis(800)), "⠋");
    }
}
