//! The colours every view of a session is drawn in, so the app and the
//! MCP server's charts agree: each reading has one colour wherever it is
//! drawn, and none of the readings' colours is used for anything else.
//! Only the readings (and the position) have colours: everything else on
//! a plot is a grey.
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
    /// The watch's position, wherever it is shown: the paper strip's
    /// track, the brush and the Positions Test.
    pub positions: Rgb,
    /// Red means Bad, on an icon only: a failure's icon, with its words in
    /// the text colour. Never on a reading, and never on text. A warning is
    /// a triangle in the text colour, so it has no colour of its own.
    pub bad: Rgb,
    /// The dot beside "Listening to …".
    pub live: Rgb,
    /// The marks on the Tick Tock profile: greys, told apart by their
    /// line styles and labels, so only the readings have colours on a plot.
    pub unlock: Rgb,
    pub drop: Rgb,
    pub peak: Rgb,
    /// The three sounds on the Tick Tock profile, a grey.
    pub sound: Rgb,
    /// Amplitude, rate and beat error wherever they are drawn: over the
    /// strip, on their charts, in their distributions and in Steadiness.
    pub trace_amplitude: Rgb,
    pub trace_rate: Rgb,
    pub trace_beat_error: Rgb,
    /// A calibre's wheels where their turns are marked on a period axis:
    /// the faint grey, drawn dotted and named, so a wheel's line never
    /// reads as a reading.
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
    tick: rgb(0x6eb9fe),
    tock: rgb(0xca6b35),
    positions: rgb(0x16f2c4),
    bad: rgb(0xff453a),
    live: rgb(0x30d158),
    unlock: rgb(0xa1a1a6),
    drop: rgb(0xa1a1a6),
    peak: rgb(0xa1a1a6),
    sound: rgb(0xa1a1a6),
    trace_amplitude: rgb(0x9e6bff),
    trace_rate: rgb(0xe140a1),
    trace_beat_error: rgb(0xf6b753),
    wheel: rgb(0x6e6e73),
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
    tick: rgb(0x1476bf),
    tock: rgb(0x8f3e03),
    positions: rgb(0x03a483),
    bad: rgb(0xd70015),
    live: rgb(0x248a3d),
    unlock: rgb(0x6e6e73),
    drop: rgb(0x6e6e73),
    peak: rgb(0x6e6e73),
    sound: rgb(0x6e6e73),
    trace_amplitude: rgb(0x8b3df8),
    trace_rate: rgb(0xb80c7e),
    trace_beat_error: rgb(0xbc8422),
    wheel: rgb(0x9a9aa0),
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

    /// OKLab, from sRGB (Björn Ottosson's matrices).
    fn oklab(c: Rgb) -> [f64; 3] {
        let lin = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (lin(c.r), lin(c.g), lin(c.b));
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        [
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        ]
    }

    /// OKLab ΔE × 100.
    fn apart(a: Rgb, b: Rgb) -> f64 {
        let (a, b) = (oklab(a), oklab(b));
        100.0 * ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    #[test]
    fn the_data_colours_are_told_apart_in_both_themes() {
        for (theme, p) in [("dark", &DARK), ("light", &LIGHT)] {
            let data = [
                ("rate", p.trace_rate),
                ("amplitude", p.trace_amplitude),
                ("beat error", p.trace_beat_error),
                ("tick", p.tick),
                ("tock", p.tock),
                ("positions", p.positions),
            ];
            for (i, (an, a)) in data.iter().enumerate() {
                for (bn, b) in &data[i + 1..] {
                    let d = apart(*a, *b);
                    assert!(d >= 15.0, "{theme}: {an} and {bn} are {d:.1} apart");
                }
                for (bn, b) in [("bad", p.bad), ("live", p.live), ("accent", p.accent)] {
                    let d = apart(*a, b);
                    assert!(d >= 8.0, "{theme}: {an} and {bn} are {d:.1} apart");
                }
            }
        }
    }

    #[test]
    fn colours_read_as_css() {
        assert_eq!(DARK.trace_rate.css(), "#e140a1");
        assert_eq!(LIGHT.tick.css(), "#1476bf");
    }
}
