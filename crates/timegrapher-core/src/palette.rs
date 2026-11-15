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
    /// Amplitude, rate and beat error wherever they are drawn: over the
    /// strip, on their charts, in their distributions and in Steadiness.
    pub trace_amplitude: Rgb,
    pub trace_rate: Rgb,
    pub trace_beat_error: Rgb,
    /// The lines on a plot by strength, weakest first ("Line Strength on
    /// Plots"): the grid and the axes (the left and bottom edges, and a
    /// zero line) are the text colour at these opacities, see-through so
    /// they keep their strength over a tint; call-outs (named and
    /// reference lines, such as the Tick Tock profile's Unlock, Drop, Peak
    /// and three sounds, and a calibre's wheels on a period axis) are one
    /// grey, told apart by pattern and label, so only the readings have
    /// colours on a plot. Data takes its reading's colour.
    pub grid_opacity: f32,
    pub axis_opacity: f32,
    pub callout: Rgb,
    /// The watch case behind the Dynamic Poise dial (#1762), fainter than
    /// the grid: its face and its outline, the text colour at these
    /// opacities over the pane.
    pub case_face_opacity: f32,
    pub case_line_opacity: f32,
}

const fn rgb(hex: u32) -> Rgb {
    Rgb::hex(hex)
}

/// The call-out grey of each theme: the profile's marks and sounds and
/// every named or reference line on a plot.
const CALLOUT_DARK: Rgb = rgb(0xa1a1a6);
const CALLOUT_LIGHT: Rgb = rgb(0x6e6e73);

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
    trace_amplitude: rgb(0x9e6bff),
    trace_rate: rgb(0xe140a1),
    trace_beat_error: rgb(0xf6b753),
    grid_opacity: 0.08,
    axis_opacity: 0.27,
    callout: CALLOUT_DARK,
    // Hugh's #262629 face and #2e2e32 outline on the #232326 pane.
    case_face_opacity: 0.015,
    case_line_opacity: 0.052,
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
    trace_amplitude: rgb(0x8b3df8),
    trace_rate: rgb(0xb80c7e),
    trace_beat_error: rgb(0xbc8422),
    grid_opacity: 0.09,
    axis_opacity: 0.33,
    callout: CALLOUT_LIGHT,
    // The same steps from the light pane: the face about a quarter of
    // the way to the grid, the outline about two thirds.
    case_face_opacity: 0.022,
    case_line_opacity: 0.06,
};

impl Palette {
    /// The text colour at opacity `a` laid over the pane, as one opaque
    /// colour: for a shape drawn over another that must hide it.
    pub fn over_card(&self, a: f32) -> Rgb {
        self.over(self.card, a)
    }

    /// The text colour at opacity `a` laid over `back`, as one opaque
    /// colour.
    pub fn over(&self, back: Rgb, a: f32) -> Rgb {
        let mix = |c: u8, t: u8| (f32::from(c) + (f32::from(t) - f32::from(c)) * a).round() as u8;
        Rgb {
            r: mix(back.r, self.text.r),
            g: mix(back.g, self.text.g),
            b: mix(back.b, self.text.b),
        }
    }

    /// The poise dial's case (#1762): its face and its outline, opaque,
    /// over the card (the MCP view's dial).
    pub fn case_face(&self) -> Rgb {
        self.over_card(self.case_face_opacity)
    }

    pub fn case_line(&self) -> Rgb {
        self.over_card(self.case_line_opacity)
    }

    /// The case's face and outline over `back`, the colour actually
    /// behind the dial (Hugh on #1772: the app's dial is on the window,
    /// not a card), the same small steps toward the text as over the card.
    pub fn case_over(&self, back: Rgb) -> (Rgb, Rgb) {
        (
            self.over(back, self.case_face_opacity),
            self.over(back, self.case_line_opacity),
        )
    }
}

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

/// The time colour map: [`viridis`] with its OKLab lightness rescaled
/// from viridis's own range to 0.60–0.92 in the dark theme and 0.30–0.64
/// in the light, its hue and chroma (OKLab a and b) kept, so its start
/// stands off a dark plot and its end off a light one (Hugh's M3, Peter
/// 11-05). What every time-coloured mark calls: the Isochronism walk and
/// the Cycle view.
pub fn time_colour(t: f32, dark: bool) -> Rgb {
    let (lo, hi) = if dark { (0.60, 0.92) } else { (0.30, 0.64) };
    let [first, last] =
        [VIRIDIS[0], VIRIDIS[VIRIDIS.len() - 1]].map(|[r, g, b]| oklab(Rgb { r, g, b })[0]);
    let mut c = oklab(viridis(t));
    c[0] = lo + (c[0] - first) / (last - first) * (hi - lo);
    from_oklab(c)
}

/// How many points [`time_table`] reads the map at: drawn linearly
/// between them, a surface is within three units a channel (rounding to
/// whole units in both the table and the map) of
/// [`time_colour`].
pub const TIME_TABLE_POINTS: usize = 129;

/// [`time_colour`] read at [`TIME_TABLE_POINTS`] evenly spaced points
/// from 0 to 1, for a surface that draws the map from a table (the MCP
/// view) rather than calling it.
pub fn time_table(dark: bool) -> Vec<Rgb> {
    (0..TIME_TABLE_POINTS)
        .map(|i| time_colour(i as f32 / (TIME_TABLE_POINTS - 1) as f32, dark))
        .collect()
}

/// sRGB's transfer, one channel 0–255 to linear 0–1.
fn to_linear(v: u8) -> f64 {
    let v = v as f64 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// A linear channel 0–1 back to sRGB 0–255, clamped.
fn from_linear(c: f64) -> u8 {
    let c = if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// OKLab, from sRGB (Björn Ottosson's matrices).
pub fn oklab(c: Rgb) -> [f64; 3] {
    let (r, g, b) = (to_linear(c.r), to_linear(c.g), to_linear(c.b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

/// sRGB from OKLab, each channel clamped into range.
fn from_oklab([l, a, b]: [f64; 3]) -> Rgb {
    let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s_ = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    Rgb {
        r: from_linear(4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_),
        g: from_linear(-1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_),
        b: from_linear(-0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_),
    }
}

#[cfg(test)]
mod tests {

    /// The poise dial's case is Hugh's steps over each pane (#1762): a
    /// face and an outline fainter than the grid.
    #[test]
    fn the_poise_case_is_fainter_than_the_grid() {
        let near = |a: Rgb, b: Rgb| {
            [(a.r, b.r), (a.g, b.g), (a.b, b.b)]
                .iter()
                .all(|&(x, y)| x.abs_diff(y) <= 1)
        };
        assert!(
            near(DARK.case_face(), Rgb::hex(0x262629)),
            "{:?}",
            DARK.case_face()
        );
        assert!(
            near(DARK.case_line(), Rgb::hex(0x2e2e32)),
            "{:?}",
            DARK.case_line()
        );
        for p in [&DARK, &LIGHT] {
            assert!(p.case_face_opacity < p.case_line_opacity);
            assert!(p.case_line_opacity < p.grid_opacity);
            // Over the window, a step off it toward the text, the outline
            // nearer the window than the grid's line is.
            let (face, line) = p.case_over(p.window);
            let off = |c: Rgb| p.window.r.abs_diff(c.r);
            assert!(off(face) >= 1 && off(face) < off(line), "{face:?} {line:?}");
            assert!(off(line) < off(p.separator), "{line:?}");
            assert_eq!(p.case_over(p.card), (p.case_face(), p.case_line()));
        }
    }

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

    /// The table read between its points stays within three units a channel
    /// of the map.
    #[test]
    fn the_time_table_is_the_map_between_its_points() {
        for dark in [true, false] {
            let table = time_table(dark);
            assert_eq!(table.len(), TIME_TABLE_POINTS);
            for k in 0..=1000 {
                let t = k as f32 / 1000.0;
                let x = t * (TIME_TABLE_POINTS - 1) as f32;
                let i = (x.floor() as usize).min(TIME_TABLE_POINTS - 2);
                let f = x - i as f32;
                let (a, b, c) = (table[i], table[i + 1], time_colour(t, dark));
                let lerp = |p: u8, q: u8| p as f32 + (q as f32 - p as f32) * f;
                for (got, want) in [
                    (lerp(a.r, b.r), c.r),
                    (lerp(a.g, b.g), c.g),
                    (lerp(a.b, b.b), c.b),
                ] {
                    assert!(
                        (got - want as f32).abs() <= 3.0,
                        "{dark} {t}: {got} vs {want}"
                    );
                }
            }
        }
    }

    /// The time map's lightness runs over its theme's range, start to end,
    /// steadily, with viridis's hue kept (Hugh's M3).
    #[test]
    fn the_time_map_is_viridis_lifted_into_each_theme() {
        for (dark, lo, hi) in [(true, 0.60, 0.92), (false, 0.30, 0.64)] {
            let l = |t: f32| oklab(time_colour(t, dark))[0];
            assert!((l(0.0) - lo).abs() < 0.01, "{dark}: {}", l(0.0));
            assert!((l(1.0) - hi).abs() < 0.01, "{dark}: {}", l(1.0));
            let steps: Vec<f64> = (0..=16).map(|i| l(i as f32 / 16.0)).collect();
            assert!(steps.windows(2).all(|w| w[1] > w[0]), "{dark}: {steps:?}");
            // Still purple to yellow: the hue angle of viridis at each end.
            for t in [0.0, 1.0] {
                let ([_, a, b], [_, va, vb]) = (oklab(time_colour(t, dark)), oklab(viridis(t)));
                let turn = (b.atan2(a) - vb.atan2(va)).abs();
                assert!(turn < 0.35, "{dark} {t}: {turn}");
            }
        }
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
