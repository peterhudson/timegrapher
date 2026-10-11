//! The colours every view of a session is drawn in, so the app and the
//! MCP server's charts agree: each reading has one colour wherever it is
//! drawn, and none of the readings' colours is used for anything else.
//! The app's theme is built from these.

/// One colour, as 8-bit sRGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn hex(hex: u32) -> Self {
        Rgb {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
        }
    }

    /// `#rrggbb`, for CSS and SVG.
    pub fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// The colours of one theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Behind everything: the window, the sidebar, the gaps between cards.
    pub window: Rgb,
    /// Cards, and the plot area of every chart.
    pub card: Rgb,
    /// Buttons, fields and the track of a segmented control.
    pub control: Rgb,
    pub control_hover: Rgb,
    pub control_active: Rgb,
    /// The selected segment of a segmented control.
    pub raised: Rgb,
    /// Hairlines between areas.
    pub separator: Rgb,
    pub text: Rgb,
    pub text_secondary: Rgb,
    pub text_tertiary: Rgb,
    pub accent: Rgb,
    /// Text on the accent colour (and on red).
    pub on_accent: Rgb,
    /// Tick (Side A, the even beats) and Tock (Side B, the odd beats).
    pub tick: Rgb,
    pub tock: Rgb,
    /// Good, warning and bad, as on the level meter and the verdicts.
    pub good: Rgb,
    pub warn: Rgb,
    pub bad: Rgb,
    /// The marks on the Tick Tock profile.
    pub unlock: Rgb,
    pub drop: Rgb,
    pub peak: Rgb,
    /// The three sounds on the Tick Tock profile.
    pub sound: Rgb,
    /// Amplitude, rate and beat error wherever they are drawn: over the
    /// strip, on their charts, in their distributions and in Steadiness.
    pub trace_amplitude: Rgb,
    pub trace_rate: Rgb,
    pub trace_beat_error: Rgb,
}

const fn rgb(hex: u32) -> Rgb {
    Rgb::hex(hex)
}

pub const DARK: Palette = Palette {
    window: rgb(0x161618),
    card: rgb(0x232326),
    control: rgb(0x343438),
    control_hover: rgb(0x3e3e43),
    control_active: rgb(0x4a4a50),
    raised: rgb(0x5a5a60),
    separator: rgb(0x303034),
    text: rgb(0xf5f5f7),
    text_secondary: rgb(0xa1a1a6),
    text_tertiary: rgb(0x6e6e73),
    accent: rgb(0x0a84ff),
    on_accent: rgb(0xffffff),
    tick: rgb(0x64d2ff),
    tock: rgb(0xff9f0a),
    good: rgb(0x30d158),
    warn: rgb(0xffb340),
    bad: rgb(0xff453a),
    unlock: rgb(0x30d158),
    drop: rgb(0x7d7aff),
    peak: rgb(0x7d7aff),
    sound: rgb(0xac8e68),
    trace_amplitude: rgb(0xbf5af2),
    trace_rate: rgb(0xff453a),
    trace_beat_error: rgb(0xffd60a),
};

pub const LIGHT: Palette = Palette {
    window: rgb(0xf2f2f5),
    card: rgb(0xffffff),
    control: rgb(0xe9e9ee),
    control_hover: rgb(0xdedee4),
    control_active: rgb(0xd1d1d8),
    raised: rgb(0xffffff),
    separator: rgb(0xdcdce1),
    text: rgb(0x1d1d1f),
    text_secondary: rgb(0x6e6e73),
    text_tertiary: rgb(0x9a9aa0),
    accent: rgb(0x007aff),
    on_accent: rgb(0xffffff),
    tick: rgb(0x0068c9),
    tock: rgb(0xc45100),
    good: rgb(0x248a3d),
    warn: rgb(0xb25000),
    bad: rgb(0xd70015),
    unlock: rgb(0x248a3d),
    drop: rgb(0x5856d6),
    peak: rgb(0x5856d6),
    sound: rgb(0x7f6545),
    trace_amplitude: rgb(0x8944ab),
    trace_rate: rgb(0xd70015),
    trace_beat_error: rgb(0x8a6d00),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_as_css() {
        assert_eq!(DARK.trace_rate.css(), "#ff453a");
        assert_eq!(LIGHT.tick.css(), "#0068c9");
    }
}
