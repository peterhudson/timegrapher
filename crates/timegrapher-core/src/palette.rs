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
    /// The app's own warnings and errors (a banner, a failed save). Never
    /// on a reading: readings carry no good or bad colour.
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
    /// A calibre's wheels where their turns are marked on a period axis:
    /// a colour of its own, so a wheel's line never reads as the grid or
    /// as a reading. Kept well away from beat error's yellow (a mint, not
    /// a lime): the lime it once was read too close to the yellow.
    pub wheel: Rgb,
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
    warn: rgb(0xffb340),
    bad: rgb(0xff453a),
    unlock: rgb(0x30d158),
    drop: rgb(0x7d7aff),
    peak: rgb(0x7d7aff),
    sound: rgb(0xac8e68),
    trace_amplitude: rgb(0xbf5af2),
    trace_rate: rgb(0xff453a),
    trace_beat_error: rgb(0xffd60a),
    wheel: rgb(0x3ddcaa),
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
    warn: rgb(0xb25000),
    bad: rgb(0xd70015),
    unlock: rgb(0x248a3d),
    drop: rgb(0x5856d6),
    peak: rgb(0x5856d6),
    sound: rgb(0x7f6545),
    trace_amplitude: rgb(0x8944ab),
    trace_rate: rgb(0xd70015),
    trace_beat_error: rgb(0x8a6d00),
    wheel: rgb(0x0b8466),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// The time-frequency maps' colours, from the smallest size to the
/// largest: viridis at 17 evenly spaced points, from matplotlib's table.
/// The same in both themes.
pub const VIRIDIS: [[u8; 3]; 17] = [
    [68, 1, 84],
    [72, 24, 106],
    [71, 45, 123],
    [66, 64, 134],
    [59, 82, 139],
    [51, 99, 141],
    [44, 114, 142],
    [38, 130, 142],
    [33, 145, 140],
    [31, 160, 136],
    [40, 174, 128],
    [63, 188, 115],
    [94, 201, 98],
    [132, 212, 75],
    [173, 220, 48],
    [216, 226, 25],
    [253, 231, 37],
];

/// Viridis at `t`, 0 to 1 (clamped), between the table's points.
pub fn viridis(t: f32) -> Rgb {
    let x = t.clamp(0.0, 1.0) * (VIRIDIS.len() - 1) as f32;
    let i = (x.floor() as usize).min(VIRIDIS.len() - 2);
    let f = x - i as f32;
    let [a, b] = [VIRIDIS[i], VIRIDIS[i + 1]];
    let mix = |k: usize| (a[k] as f32 + (b[k] as f32 - a[k] as f32) * f).round() as u8;
    Rgb {
        r: mix(0),
        g: mix(1),
        b: mix(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viridis_runs_from_purple_to_yellow_between_its_points() {
        assert_eq!(viridis(0.0).css(), "#440154");
        assert_eq!(viridis(1.0).css(), "#fde725");
        assert_eq!(viridis(-1.0), viridis(0.0));
        assert_eq!(viridis(2.0), viridis(1.0));
        // Halfway between the first two points.
        assert_eq!(
            viridis(1.0 / 32.0),
            Rgb {
                r: 70,
                g: 13,
                b: 95
            }
        );
    }

    #[test]
    fn colours_read_as_css() {
        assert_eq!(DARK.trace_rate.css(), "#ff453a");
        assert_eq!(LIGHT.tick.css(), "#0068c9");
    }
}
