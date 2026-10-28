//! The look of the app: colours for the light and dark themes, the type
//! scale, spacing, and a few widgets that egui doesn't have (cards, section
//! headers, a segmented control, switches and chips).
//!
//! Colour carries meaning and nothing else: each reading has its colour
//! everywhere (rate magenta, amplitude violet, beat error amber, Tick blue,
//! Tock deep orange) and the position is teal. Red is a failure's icon,
//! green the Listening dot, and one accent marks what can be pressed or is
//! selected. The rest is greys: a warning is a triangle in the text colour.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame,
    Layout, Margin, Response, RichText, Sense, Shadow, Stroke, TextStyle, Theme, Ui, Vec2,
};
use std::sync::Arc;
use timegrapher_core::palette as core_palette;

/// The colours of one theme, from [`timegrapher_core::palette`].
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Behind everything: the window, the menu bar, the gaps between cards.
    pub window: Color32,
    /// Cards: the readings, the panes, the groups of settings.
    pub card: Color32,
    /// Buttons, fields and the track of a segmented control.
    pub control: Color32,
    pub control_hover: Color32,
    pub control_active: Color32,
    /// The selected segment of a segmented control.
    pub raised: Color32,
    /// Hairlines between areas.
    pub separator: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_tertiary: Color32,
    pub accent: Color32,
    /// Text on the accent colour (and on Bad's red icon).
    pub on_accent: Color32,
    pub tick: Color32,
    pub tock: Color32,
    /// The watch's position: the track, the brush and the Positions Test.
    pub positions: Color32,
    /// A failure's icon, never text.
    pub bad: Color32,
    /// The dot beside "Listening to …".
    pub live: Color32,
    /// The marks on the tick tock profile: greys, told apart by line style.
    pub unlock: Color32,
    pub drop: Color32,
    pub peak: Color32,
    /// The three sounds on the tick tock profile.
    pub sound: Color32,
    /// A calibre's wheels, where their turns are marked on a period axis.
    pub wheel: Color32,
    /// Amplitude, rate and beat error wherever they are drawn: over the
    /// strip, on their charts, in their distributions and in Steadiness.
    /// Each colour means one thing across the app, so nothing else on a
    /// plot is coloured.
    pub trace_amplitude: Color32,
    pub trace_rate: Color32,
    pub trace_beat_error: Color32,
    /// How strongly the paper strip's track shows the position, as
    /// fractions of [`Palette::positions`]: `track[0]` through the settling
    /// after each change, then fading over [`TRACK_FADE_S`] from
    /// `track[1]` to `track[2]`, where it stays. Stronger in light mode,
    /// where teal on white is fainter.
    pub track: [f32; 3],
}

pub const TRACK_FADE_S: f64 = 40.0;
/// The plot behind the beats takes the track at this much of its strength.
pub const TRACK_WASH: f32 = 1.0 / 6.0;
/// Words and codes drawn on solid teal (the brush's swatch, a lit
/// position button): dark in both themes.
pub const ON_TEAL: Color32 = Color32::from_rgb(0x1d, 0x1d, 0x1f);
/// A masked stretch is hatched in the faint text colour: on the track at
/// full strength, down the plot at [`MASK_WASH`] of it.
pub const MASK_WASH: f32 = 0.35;

const fn c(x: core_palette::Rgb) -> Color32 {
    Color32::from_rgb(x.r, x.g, x.b)
}

impl Palette {
    /// The colour to write words in for something drawn in `c`: `c`
    /// itself, except where it is too faint for small text (beat error's
    /// amber and the position's teal in light mode), which take the text
    /// colour beside a mark in their own.
    pub fn ink(&self, c: Color32) -> Color32 {
        let faint = self.text == LIGHT.text && (c == self.trace_beat_error || c == self.positions);
        if faint {
            self.text
        } else {
            c
        }
    }
}

/// The core palette (shared with the MCP server's charts) in egui's colours.
const fn from_core(p: &core_palette::Palette, track: [f32; 3]) -> Palette {
    Palette {
        window: c(p.window),
        card: c(p.card),
        control: c(p.control),
        control_hover: c(p.control_hover),
        control_active: c(p.control_active),
        raised: c(p.raised),
        separator: c(p.separator),
        text: c(p.text),
        text_secondary: c(p.text_secondary),
        text_tertiary: c(p.text_tertiary),
        accent: c(p.accent),
        on_accent: c(p.on_accent),
        tick: c(p.tick),
        tock: c(p.tock),
        positions: c(p.positions),
        bad: c(p.bad),
        live: c(p.live),
        unlock: c(p.unlock),
        drop: c(p.drop),
        peak: c(p.peak),
        sound: c(p.sound),
        wheel: c(p.wheel),
        trace_amplitude: c(p.trace_amplitude),
        trace_rate: c(p.trace_rate),
        trace_beat_error: c(p.trace_beat_error),
        track,
    }
}

pub const DARK: Palette = from_core(&core_palette::DARK, [0.62, 0.48, 0.24]);
pub const LIGHT: Palette = from_core(&core_palette::LIGHT, [0.80, 0.62, 0.36]);

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// The palette of the theme `ui` is drawn in.
pub fn pal(ui: &Ui) -> &'static Palette {
    palette(ui.visuals().dark_mode)
}

/// Corner radius of cards and of buttons and fields.
pub const CARD_RADIUS: u8 = 10;
pub const CONTROL_RADIUS: u8 = 6;
/// Space between cards, and inside them.
pub const GAP: f32 = 12.0;
pub const CARD_PADDING: i8 = 16;

/// The readings cards' three rows, 8 apart: the title, the figure and the
/// controls. Every control on them is `CONTROL_H` tall.
pub const TITLE_ROW_H: f32 = 18.0;
pub const FIGURE_ROW_H: f32 = 40.0;
pub const CONTROL_H: f32 = 28.0;
pub const ROW_GAP: f32 = 8.0;
/// A readings card's height at every width, whatever it shows: its three
/// rows and its padding.
pub const READINGS_CARD_H: f32 =
    TITLE_ROW_H + FIGURE_ROW_H + CONTROL_H + 2.0 * ROW_GAP + 2.0 * CARD_PADDING as f32;
/// The size of a readings card's big figure, and on a compact card.
pub const FIGURE_PT: f32 = 32.0;
pub const FIGURE_COMPACT_PT: f32 = 26.0;

const SEMIBOLD: &str = "semibold";
const DISPLAY: &str = "display";

/// Semibold text for headings and emphasis.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// The large figures of the readings.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    for (name, bytes) in [
        (
            "inter",
            &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..],
        ),
        (
            "inter-semibold",
            &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..],
        ),
        (
            "inter-display",
            &include_bytes!("../assets/fonts/InterDisplay-SemiBold.ttf")[..],
        ),
    ] {
        f.font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    // egui's own fonts stay behind Inter for anything it lacks.
    let fallback = f
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let with = |first: &[&str]| {
        let mut v: Vec<String> = first.iter().map(|s| s.to_string()).collect();
        v.extend(fallback.iter().cloned());
        v
    };
    f.families
        .insert(FontFamily::Proportional, with(&["inter"]));
    f.families
        .insert(FontFamily::Name(SEMIBOLD.into()), with(&["inter-semibold"]));
    f.families.insert(
        FontFamily::Name(DISPLAY.into()),
        with(&["inter-display", "inter-semibold"]),
    );
    f
}

fn visuals(p: &Palette, dark: bool) -> egui::Visuals {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let r = CornerRadius::same(CONTROL_RADIUS);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.weak_bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.corner_radius = r;
    for (wv, fill) in [
        (&mut w.inactive, p.control),
        (&mut w.hovered, p.control_hover),
        (&mut w.active, p.control_active),
        (&mut w.open, p.control_hover),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::NONE;
        wv.fg_stroke = Stroke::new(1.5_f32, p.text);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    // A hovered field or checkbox shows a ring in the accent colour.
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent.gamma_multiply(0.6));
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.5_f32, p.on_accent);
    v.hyperlink_color = p.accent;
    v.weak_text_color = Some(p.text_secondary);
    v.faint_bg_color = p.control;
    v.extreme_bg_color = p.card;
    v.text_edit_bg_color = Some(p.control);
    v.code_bg_color = p.control;
    v.warn_fg_color = p.text;
    v.error_fg_color = p.bad;
    v.panel_fill = p.window;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0_f32, p.separator);
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    v.menu_corner_radius = CornerRadius::same(8);
    let shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 40 }),
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.indent_has_left_vline = false;
    v.striped = false;
    v
}

fn style(style: &mut egui::Style, dark: bool) {
    let p = palette(dark);
    style.visuals = visuals(p, dark);
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Heading, semibold(17.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(10.0, 4.0);
    s.interact_size = Vec2::new(24.0, 24.0);
    s.slider_rail_height = 4.0;
    s.combo_height = 300.0;
    s.menu_margin = Margin::same(6);
    s.window_margin = Margin::same(10);
    s.tooltip_width = 320.0;
    s.icon_width = 16.0;
    s.icon_width_inner = 9.0;
}

/// Install the fonts and both themes' styles.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.style_mut_of(Theme::Dark, |s| style(s, true));
    ctx.style_mut_of(Theme::Light, |s| style(s, false));
}

/// A rounded card in the card colour.
pub fn card() -> Frame {
    Frame::new()
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::same(CARD_PADDING))
}

/// A small, spaced, upper-case caption over a group of settings or a
/// reading.
pub fn caption(text: &str) -> RichText {
    RichText::new(text).font(semibold(12.5))
}

/// A small round "?" whose explanation shows while the pointer is on it,
/// and stays open once it's clicked, until a click anywhere else. The one way the app explains itself at length;
/// short hints stay as tooltips on the controls.
pub fn help(ui: &mut Ui, title: &str, paragraphs: &[&str]) -> Response {
    help_with_key(ui, title, paragraphs, &[])
}

/// [`help`], ending with a key to a toolbar: each icon drawn beside its
/// name, so a picture that can't be guessed can be looked up.
pub fn help_with_key(
    ui: &mut Ui,
    title: &str,
    paragraphs: &[&str],
    key: &[(Icon, String)],
) -> Response {
    let p = pal(ui);
    let size = Vec2::splat(16.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let fill = if open {
        p.accent
    } else if resp.hovered() {
        p.control_active
    } else {
        p.control_hover
    };
    ui.painter().circle_filled(rect.center(), 8.0, fill);
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        "?",
        semibold(11.0),
        if open { p.on_accent } else { p.text_secondary },
    );
    // Hovering shows the first paragraph, the plain answer, so it covers
    // little; a click shows it all.
    let body = |ui: &mut Ui, pinned: bool| {
        ui.set_max_width(360.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(RichText::new(title).font(semibold(14.0)));
        let shown = if pinned { paragraphs.len() } else { 1 };
        for para in &paragraphs[..shown.min(paragraphs.len())] {
            ui.add(egui::Label::new(RichText::new(*para).size(12.5)).wrap());
        }
        if pinned && !key.is_empty() {
            ui.label(RichText::new("The Toolbar's Icons").font(semibold(12.5)));
            let pal = pal(ui);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                for (icon, name) in key {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        crate::icons::draw(ui.painter(), r, *icon, pal.text_secondary);
                        ui.label(RichText::new(name).size(12.5));
                    });
                }
            });
        }
        if !pinned {
            ui.label(
                RichText::new(if paragraphs.len() > 1 {
                    "Click the ? for more."
                } else {
                    "Click the ? to keep this open."
                })
                .small()
                .color(pal(ui).text_tertiary),
            );
        }
    };
    // Hovering shows the start and moving away hides it; a click keeps
    // all of it open until a click anywhere else.
    let resp = if open {
        resp
    } else {
        resp.on_hover_ui(|ui| body(ui, false))
    };
    // Below the "?", or above it where there is more room there (a pane
    // at the window's foot).
    let screen = ui.ctx().content_rect();
    let below = screen.bottom() - resp.rect.bottom() - 16.0;
    let above = resp.rect.top() - screen.top() - 16.0;
    let up = below < 240.0 && above > below;
    const DOWN: [egui::RectAlign; 2] = [egui::RectAlign::BOTTOM_START, egui::RectAlign::BOTTOM_END];
    const UP: [egui::RectAlign; 2] = [egui::RectAlign::TOP_START, egui::RectAlign::TOP_END];
    let sides: &[egui::RectAlign] = if up { &UP } else { &DOWN };
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(360.0)
        .align(sides[0])
        .align_alternatives(sides)
        .show(|ui| {
            // A long explanation and its key of icons can be taller than
            // the room on its side of the "?": it scrolls from its start,
            // its bar drawn at rest, rather than running off the window's
            // edge.
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            let room = if up { above } else { below } - 24.0;
            let mut scroll = egui::ScrollArea::vertical().max_height(room.max(80.0));
            // Each time it opens, from its title.
            if !open {
                scroll = scroll.vertical_scroll_offset(0.0);
            }
            scroll.show(ui, |ui| body(ui, true));
        });
    resp
}

/// A reading's limit in short, in a small rounded chip no wider than
/// `max_w`: its whole sentences show while the pointer is on it, and stay
/// open once it's clicked, as the "?" does.
pub fn chip(ui: &mut Ui, text: &str, title: &str, sentences: &[String], max_w: f32) -> Response {
    let p = pal(ui);
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_string(),
        FontId::proportional(12.0),
        p.text_secondary,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((max_w - 14.0).max(8.0));
    let galley = ui.painter().layout_job(job);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + 14.0, 18.0), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let fill = if open {
        p.control_active
    } else if resp.hovered() {
        p.control_hover
    } else {
        p.control
    };
    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        p.text_secondary,
    );
    let body = |ui: &mut Ui| {
        ui.set_max_width(320.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(RichText::new(title).font(semibold(14.0)));
        for s in sentences {
            ui.add(egui::Label::new(RichText::new(s).size(12.5)).wrap());
        }
    };
    let resp = if open { resp } else { resp.on_hover_ui(body) };
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(320.0)
        .show(body);
    resp
}

/// A row in a settings card: the name on the left (with its hint on hover)
/// and the control after it, lined up with the rows above and below.
pub fn row<R>(
    ui: &mut Ui,
    label: &str,
    hint: Option<&str>,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    if in_toolbar(ui) {
        // In a pane's toolbar: the name in small grey type just before
        // its control, kept together. A setting that didn't fit last
        // frame starts a new line, so nothing runs off the pane's edge.
        return ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let l = ui.label(
                    RichText::new(label)
                        .size(12.0)
                        .color(pal(ui).text_secondary),
                );
                if let Some(h) = hint {
                    l.on_hover_text(h);
                }
                control(ui)
            })
            .inner;
    }
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(LABEL_W, 24.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            pal(ui).text,
        );
        if let Some(h) = hint {
            resp.on_hover_text(h);
        }
        control(ui)
    })
    .inner
}

/// Width of the names in a settings card.
pub const LABEL_W: f32 = 92.0;

/// Settings' groups are no wider than this.
pub const GROUP_W: f32 = 560.0;

/// A settings row's height, without a caption that wraps.
pub const GROUP_ROW_H: f32 = 40.0;

/// What sits under a settings row's name: its one-line caption, or a
/// message about the setting in the caption's place, so a message never
/// adds a line under the row.
#[derive(Debug, Clone, Copy)]
pub enum Caption<'a> {
    None,
    Text(&'a str),
    Alert(Alert, &'a str),
}

/// `text` cut in the middle with "…" to fit `w` points in `font`, keeping
/// its start and its end, where similar names differ, as in "(2)".
pub fn middle_cut(ui: &Ui, text: &str, font: &egui::FontId, w: f32) -> String {
    let width = |t: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
    };
    if width(text) <= w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |keep: usize| {
        let head = keep.div_ceil(2);
        let tail = keep / 2;
        let mut s: String = chars[..head].iter().collect();
        s.push('…');
        s.extend(&chars[chars.len() - tail..]);
        s
    };
    // The most characters kept that still fit.
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&cut(mid)) <= w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
}

/// A settings row's name, with its hover, and its caption under it.
fn name_and_caption(ui: &mut Ui, label: &str, caption: Caption, hint: &str) {
    let p = pal(ui);
    ui.spacing_mut().item_spacing.y = 1.0;
    ui.add(egui::Label::new(RichText::new(label).size(13.5).color(p.text)).wrap())
        .on_hover_text(hint);
    match caption {
        Caption::None => {}
        Caption::Text(t) => {
            ui.add(egui::Label::new(RichText::new(t).size(11.5).color(p.text_secondary)).wrap());
        }
        Caption::Alert(kind, t) => {
            // As tall as a caption, one line cut short, so the row
            // keeps its height; the row's hover says more.
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                alert_icon(ui.painter(), rect.translate(Vec2::new(0.0, 1.0)), kind, p);
                ui.add(
                    egui::Label::new(RichText::new(t).size(11.5).color(p.text_secondary))
                        .truncate(),
                );
            });
        }
    }
}

/// A titled group of settings: the title in small secondary text above a
/// rounded panel no wider than [`GROUP_W`], its rows apart by hairlines.
/// `help`, when given, is a "?" beside the title.
pub fn group<R>(
    ui: &mut Ui,
    title: &str,
    help_text: Option<&[&str]>,
    add: impl FnOnce(&mut Group<'_>) -> R,
) -> R {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(RichText::new(title).size(12.0).color(p.text_secondary));
        if let Some(h) = help_text {
            help(ui, title, h);
        }
    });
    let w = ui.available_width().min(GROUP_W);
    // A step off the window it sits on: lighter in the dark theme, the
    // window's grey on a white one in the light.
    let fill = if ui.visuals().dark_mode {
        lerp_color(p.card, p.control, 0.5)
    } else {
        p.window
    };
    let out = Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 0))
        .show(ui, |ui| {
            ui.set_width(w - 24.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            add(&mut Group { ui, rows: 0 })
        })
        .inner;
    ui.add_space(14.0);
    out
}

/// The rows of a [`group`].
pub struct Group<'a> {
    pub ui: &'a mut Ui,
    rows: usize,
}

impl Group<'_> {
    /// A hairline above each row but the first.
    fn next(&mut self) {
        if self.rows > 0 {
            let r = self.ui.available_rect_before_wrap();
            let y = r.top();
            // Darker than the group in the dark theme; the separator grey
            // on the light theme's.
            let p = pal(self.ui);
            let ink = if self.ui.visuals().dark_mode {
                p.card
            } else {
                p.separator
            };
            self.ui
                .painter()
                .hline(r.x_range(), y, Stroke::new(1.0_f32, ink));
        }
        self.rows += 1;
    }

    /// A row: the name at the left with its caption under it, and the
    /// control at the right of the same row; `hint` on the name.
    pub fn row<R>(
        &mut self,
        label: &str,
        caption: Caption,
        hint: &str,
        control: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.next();
        let ui = &mut *self.ui;
        let w = ui.available_width();
        ui.allocate_ui_with_layout(
            Vec2::new(w, GROUP_ROW_H),
            Layout::right_to_left(Align::Center),
            |ui| {
                ui.set_min_size(Vec2::new(w, GROUP_ROW_H));
                ui.spacing_mut().item_spacing.x = 8.0;
                // The control laid left to right at the right edge, in the
                // width it took last frame, so a row of buttons or a
                // segmented control keeps its order.
                let id = ui.id().with(("group row", label));
                let was = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
                let r = ui.allocate_ui_with_layout(
                    Vec2::new(was, GROUP_ROW_H),
                    Layout::left_to_right(Align::Center),
                    control,
                );
                let took = r.response.rect.width();
                if (took - was).abs() > 0.5 {
                    ui.ctx().data_mut(|d| d.insert_temp(id, took));
                    ui.ctx().request_repaint();
                }
                let out = r.inner;
                ui.add_space(8.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    // A name alone sits level with its control.
                    ui.add_space(if matches!(caption, Caption::None) {
                        11.0
                    } else {
                        6.0
                    });
                    name_and_caption(ui, label, caption, hint);
                    ui.add_space(6.0);
                });
                out
            },
        )
        .inner
    }

    /// A row with its control on its own line under the name and caption,
    /// across the group: for a control whose words vary in length, such as
    /// a device's name, so the row never moves as they change.
    pub fn row_under<R>(
        &mut self,
        label: &str,
        caption: Caption,
        hint: &str,
        control: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.next();
        let ui = &mut *self.ui;
        ui.add_space(6.0);
        let out = ui
            .scope(|ui| {
                name_and_caption(ui, label, caption, hint);
                ui.add_space(6.0);
                ui.horizontal(control).inner
            })
            .inner;
        ui.add_space(8.0);
        out
    }

    /// A row of its own: whatever `add` draws, across the group.
    pub fn whole<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.next();
        let ui = &mut *self.ui;
        ui.add_space(8.0);
        let out = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                add(ui)
            })
            .inner;
        ui.add_space(8.0);
        out
    }
}

/// A row with a name on the left and a switch on the right. True when the
/// switch was flipped.
pub fn switch_row(ui: &mut Ui, label: &str, on: &mut bool, hint: &str) -> bool {
    if in_toolbar(ui) {
        // In a pane's toolbar a switch is a button that stays pressed
        // while it is on.
        return toggle(ui, label, on).on_hover_text(hint).changed();
    }
    ui.horizontal(|ui| {
        let l = ui.add(egui::Label::new(label).sense(Sense::click()));
        let flip_by_label = l.clicked();
        l.on_hover_text(hint);
        let r = ui
            .with_layout(Layout::right_to_left(Align::Center), |ui| switch(ui, on))
            .inner
            .on_hover_text(hint);
        if flip_by_label {
            *on = !*on;
        }
        r.changed() || flip_by_label
    })
    .inner
}

/// An on/off switch.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let size = Vec2::new(34.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    if ui.is_rect_visible(rect) {
        let p = pal(ui);
        let t = ui.ctx().animate_bool_responsive(resp.id, *on);
        let off = if resp.hovered() {
            p.control_active
        } else {
            p.control_hover
        };
        let track = lerp_color(off, p.accent, t);
        let radius = rect.height() / 2.0;
        ui.painter().rect_filled(rect, radius, track);
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        let c = egui::pos2(x, rect.center().y);
        ui.painter().circle_filled(
            c + Vec2::new(0.0, 0.5),
            radius - 2.0,
            Color32::from_black_alpha(40),
        );
        ui.painter().circle_filled(c, radius - 2.5, Color32::WHITE);
    }
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// A segmented control: one of a few choices, the selected one raised.
/// With two choices a click anywhere on it switches to the other, so it can
/// be flipped back and forth without moving the pointer. True when the
/// choice changed.
/// A pop-up menu's item: the one in use carries a ✓ before its words,
/// and the highlight is the pointer's alone, as in a macOS pop-up.
pub fn menu_item(ui: &mut Ui, on: bool, words: impl Into<String>) -> Response {
    let words = words.into();
    let text = if on { format!("✓ {words}") } else { words };
    ui.add(egui::Button::new(text).frame_when_inactive(false))
}

pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    choices: &[(T, &str, &str)],
) -> bool {
    if in_toolbar(ui) {
        return ui.horizontal(|ui| segmented_body(ui, value, choices)).inner;
    }
    segmented_body(ui, value, choices)
}

fn segmented_body<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    choices: &[(T, &str, &str)],
) -> bool {
    let p = pal(ui);
    let mut changed = false;
    Frame::new()
        .fill(p.control)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS + 1))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.spacing_mut().button_padding = Vec2::new(10.0, 2.0);
                let two = choices.len() == 2;
                for (i, &(v, label, hint)) in choices.iter().enumerate() {
                    let selected = *value == v;
                    let text = if selected {
                        RichText::new(label).font(semibold(12.5)).color(p.text)
                    } else {
                        RichText::new(label).size(12.5).color(p.text_secondary)
                    };
                    let b = egui::Button::new(text)
                        .fill(if selected { p.raised } else { p.control })
                        .stroke(Stroke::NONE)
                        .frame_when_inactive(selected)
                        .corner_radius(CornerRadius::same(CONTROL_RADIUS - 1))
                        .min_size(Vec2::new(0.0, 20.0));
                    let mut r = ui.add(b);
                    let other = if two { choices[1 - i] } else { choices[i] };
                    if two && selected {
                        let flip = format!("Click to switch to {}", other.1);
                        r = r.on_hover_text(if hint.is_empty() {
                            flip
                        } else {
                            format!("{hint}. {flip}.")
                        });
                    } else if !hint.is_empty() {
                        r = r.on_hover_text(hint);
                    }
                    if r.clicked() {
                        if !selected {
                            *value = v;
                            changed = true;
                        } else if two {
                            *value = other.0;
                            changed = true;
                        }
                    }
                }
            });
        });
    changed
}

/// The button for the one thing to do next: filled with the accent colour.
pub fn primary(ui: &Ui, text: &str) -> egui::Button<'static> {
    let p = pal(ui);
    egui::Button::new(RichText::new(text).font(semibold(13.0)).color(p.on_accent))
        .fill(p.accent)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .min_size(Vec2::new(84.0, 26.0))
}

/// A button of the same size as the primary one, without its colour.
pub fn secondary(_ui: &Ui, text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).font(semibold(13.0)))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .min_size(Vec2::new(84.0, 26.0))
}

/// A button that stops, deletes or discards: a plain word button whose
/// word says what is lost, never filled and never red.
pub fn destructive(ui: &Ui, text: &str) -> egui::Button<'static> {
    secondary(ui, text)
}

/// What a button in a dialog's button row does, which says where it sits
/// (as a Mac dialog lays them out): the default rightmost, Cancel to its
/// left, others to the left of Cancel, and a button that throws something
/// away at the far left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Filled; Return presses it.
    Default,
    /// Escape presses it.
    Cancel,
    Other,
    /// Throws something away: far from the default.
    Destructive,
}

/// One button in a dialog's button row.
#[derive(Debug, Clone, Copy)]
pub struct DialogButton<'a> {
    pub role: Role,
    pub label: &'a str,
    pub hint: &'a str,
    /// Greyed out, saying why on hover, when not.
    pub enabled: Result<(), &'a str>,
}

impl<'a> DialogButton<'a> {
    pub fn new(role: Role, label: &'a str, hint: &'a str) -> Self {
        Self {
            role,
            label,
            hint,
            enabled: Ok(()),
        }
    }

    /// Greyed out, with `why` on hover, unless `on`.
    pub fn enabled_unless(mut self, on: bool, why: &'a str) -> Self {
        self.enabled = if on { Ok(()) } else { Err(why) };
        self
    }
}

/// A dialog's button row along its foot: each button where its role puts
/// it, all of one height. Return presses the default button (when it is
/// enabled and no field has the keyboard), Escape the Cancel button.
/// Returns the index in `buttons` of the one pressed.
pub fn dialog_buttons(ui: &mut Ui, buttons: &[DialogButton<'_>]) -> Option<usize> {
    let mut hit = None;
    let w = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(w, 26.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            hit = dialog_buttons_right(ui, buttons);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                for (i, b) in buttons.iter().enumerate() {
                    if b.role == Role::Destructive && dialog_button(ui, b) {
                        hit = Some(i);
                    }
                }
            });
        },
    );
    hit.or_else(|| dialog_keys(ui, buttons))
}

/// The right-hand part of a dialog's button row, for a row with more in
/// it, inside a right-to-left layout: the default, then Cancel, then the
/// others. Destructive buttons are left to the caller's left side.
pub fn dialog_buttons_right(ui: &mut Ui, buttons: &[DialogButton<'_>]) -> Option<usize> {
    let mut hit = None;
    for role in [Role::Default, Role::Cancel, Role::Other] {
        for (i, b) in buttons.iter().enumerate() {
            if b.role == role && dialog_button(ui, b) {
                hit = Some(i);
            }
        }
    }
    hit
}

/// Return and Escape on a dialog's button row.
pub fn dialog_keys(ui: &Ui, buttons: &[DialogButton<'_>]) -> Option<usize> {
    let free = ui.memory(|m| m.focused().is_none());
    let find = |role| {
        buttons
            .iter()
            .position(|b| b.role == role && b.enabled.is_ok())
    };
    if free && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        return find(Role::Default);
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return find(Role::Cancel);
    }
    None
}

fn dialog_button(ui: &mut Ui, b: &DialogButton<'_>) -> bool {
    let button = match b.role {
        Role::Default => primary(ui, b.label),
        Role::Destructive => destructive(ui, b.label),
        Role::Cancel | Role::Other => secondary(ui, b.label),
    };
    let r = ui.add_enabled(b.enabled.is_ok(), button);
    let r = if b.hint.is_empty() {
        r
    } else {
        r.on_hover_text(b.hint)
    };
    match b.enabled {
        Err(why) => r.on_disabled_hover_text(why).clicked(),
        Ok(()) => r.clicked(),
    }
}

/// Grid marks for a plot, all of one step so every grid line and label is
/// drawn alike: the first step of `ladder` (1, 2 and 5 times powers of ten
/// when empty) at least `min_px` points apart. 80 points is what the plot
/// needs to show every label along a horizontal axis at full strength, 30
/// along a vertical one.
pub fn even_grid(
    input: egui_plot::GridInput,
    min_px: f64,
    ladder: &[f64],
) -> Vec<egui_plot::GridMark> {
    // The plot's base step is the value of its 8-point minimum spacing.
    let want = (input.base_step_size / 8.0 * min_px).max(1e-12);
    let step = if ladder.is_empty() {
        let p = 10f64.powf(want.log10().floor());
        [1.0, 2.0, 5.0, 10.0]
            .into_iter()
            .map(|m| m * p)
            .find(|&s| s >= want * (1.0 - 1e-9))
            .unwrap_or(10.0 * p)
    } else {
        ladder
            .iter()
            .copied()
            .find(|&s| s >= want)
            .unwrap_or(*ladder.last().unwrap())
    };
    let (lo, hi) = input.bounds;
    let mut k = (lo / step).ceil();
    let mut marks = Vec::new();
    while k * step <= hi && marks.len() < 400 {
        marks.push(egui_plot::GridMark {
            value: k * step,
            step_size: step,
        });
        k += 1.0;
    }
    marks
}

/// The vertical grid of a short chart: [`even_grid`], closer together on a
/// chart too short for two labels 30 points apart, down to 14 (a small
/// label's height).
pub fn short_y_grid(input: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    let at = |px: f64| {
        let again = egui_plot::GridInput {
            bounds: input.bounds,
            base_step_size: input.base_step_size,
        };
        even_grid(again, px, &[])
    };
    [24.0, 19.0, 14.0]
        .into_iter()
        .map(at)
        .find(|m| m.len() >= 2)
        .unwrap_or_else(|| at(30.0))
}

/// An axis label, or none when its mark is within `frac` of the plot's
/// right end, where half of it would be cut off.
pub fn clear_of_right_edge(
    text: String,
    value: f64,
    range: &std::ops::RangeInclusive<f64>,
    frac: f64,
) -> String {
    let (a, b) = (*range.start(), *range.end());
    if b > a && (b - value) < frac * (b - a) {
        String::new()
    } else {
        text
    }
}

/// Steps for a time axis in seconds: whole seconds, minutes and hours.
pub const TIME_STEPS: [f64; 22] = [
    0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0,
    3600.0, 7200.0, 10800.0, 21600.0, 43200.0, 86400.0, 172800.0,
];

/// A coloured dot, the key to a line or side. Its response joins the
/// label beside it (`Response::union`), so its hover text shows over both.
pub fn dot(ui: &mut Ui, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
    resp
}

/// A short dashed line of two colours, for a key beside a line drawn
/// that way.
pub fn dash(ui: &mut Ui, a: Color32, b: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(16.0, 14.0), Sense::hover());
    let y = rect.center().y;
    for i in 0..4 {
        let x = rect.left() + i as f32 * 4.0;
        ui.painter().line_segment(
            [egui::pos2(x, y), egui::pos2(x + 4.0, y)],
            Stroke::new(2.0_f32, if i % 2 == 0 { a } else { b }),
        );
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dialog's button row as on a Mac, whatever order the buttons are
    /// given in: the default rightmost, Cancel to its left, others left of
    /// Cancel, a button that throws something away at the far left. Return
    /// presses the default (not while greyed out), Escape Cancel.
    #[test]
    fn a_dialog_row_puts_each_button_where_its_role_says() {
        let ctx = egui::Context::default();
        install(&ctx);
        let buttons = [
            DialogButton::new(Role::Destructive, "Don't Save", ""),
            DialogButton::new(Role::Cancel, "Cancel", ""),
            DialogButton::new(Role::Other, "Elsewhere…", ""),
            DialogButton::new(Role::Default, "Save…", ""),
        ];
        let run = |keys: &[egui::Key], buttons: &[DialogButton]| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(600.0, 200.0),
                )),
                events: keys
                    .iter()
                    .map(|&key| egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    })
                    .collect(),
                ..Default::default()
            };
            let mut hit = None;
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    hit = dialog_buttons(ui, buttons);
                });
            });
            let mut at = std::collections::HashMap::new();
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    at.insert(t.galley.text().to_string(), t.pos.x);
                }
            }
            (hit, at)
        };
        let (hit, at) = run(&[], &buttons);
        assert_eq!(hit, None);
        let x = |n: &str| at[n];
        assert!(x("Don't Save") < 100.0, "{at:?}");
        assert!(x("Don't Save") < x("Elsewhere…"));
        assert!(x("Elsewhere…") < x("Cancel"));
        assert!(x("Cancel") < x("Save…"));
        assert!(x("Save…") > 450.0, "{at:?}");
        assert_eq!(run(&[egui::Key::Enter], &buttons).0, Some(3));
        assert_eq!(run(&[egui::Key::Escape], &buttons).0, Some(1));
        let mut greyed = buttons;
        greyed[3] = greyed[3].enabled_unless(false, "Fill it in first");
        assert_eq!(run(&[egui::Key::Enter], &greyed).0, None);
    }

    #[test]
    fn fonts_load_and_lay_out() {
        let ctx = egui::Context::default();
        install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(RichText::new("+12.3 ±0.5 ms 275° −1").font(display(40.0)));
                ui.label(RichText::new("Tick 276°").font(semibold(13.0)));
            });
        });
    }

    #[test]
    fn a_two_way_control_flips_from_either_half() {
        // Click the selected half: it switches to the other.
        let ctx = egui::Context::default();
        install(&ctx);
        let mut v = false;
        let frame = |events: Vec<egui::Event>, v: &mut bool| {
            let mut rect = egui::Rect::NOTHING;
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(400.0, 200.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = ui.horizontal(|ui| {
                        segmented(ui, v, &[(false, "Stacked", ""), (true, "Beside", "")]);
                    });
                    rect = r.response.rect;
                });
            });
            rect
        };
        let rect = frame(vec![], &mut v);
        // The left half is "Stacked", which is selected.
        let at = egui::pos2(rect.left() + 20.0, rect.center().y);
        let click = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(at)], &mut v);
        frame(vec![click(true)], &mut v);
        frame(vec![click(false)], &mut v);
        assert!(v, "clicking the selected half switches to the other");
    }

    #[test]
    fn text_stands_out_from_its_card() {
        // Rough luminance contrast between text and card, both themes.
        let lum = |c: Color32| {
            let f = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.04045 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let contrast = |a: Color32, b: Color32| {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        for p in [&DARK, &LIGHT] {
            assert!(contrast(p.text, p.card) > 12.0);
            assert!(contrast(p.text_secondary, p.card) > 4.5);
            assert!(contrast(p.tick, p.card) > 3.0);
            assert!(contrast(p.tock, p.card) > 3.0);
            assert!(contrast(p.on_accent, p.accent) > 3.0);
        }
    }
}

/// Whether controls are being drawn into a pane's toolbar row, where they
/// sit side by side as icons rather than one above another with words.
pub fn in_toolbar(ui: &Ui) -> bool {
    bar(ui).is_some_and(|b| b.menu.is_none())
}

/// A pane toolbar being drawn: which pass it is and what it folds.
#[derive(Clone, Default)]
struct Bar {
    /// The toolbar's name, which its widths are kept under.
    title: String,
    /// `Some` while drawing the "…" menu, holding nothing; `None` for the
    /// row.
    menu: Option<()>,
    /// The controls that didn't fit the row this frame, by key.
    folded: Vec<String>,
    /// The keys met so far this pass, in order.
    met: Vec<String>,
    /// Inside a control: what it holds is drawn as it is, not folded.
    depth: u32,
    /// The keys that never fold, met so far this pass.
    keep: Vec<String>,
    /// The next control met never folds: see [`keep_next`].
    keep_next: bool,
    /// Each icon met in the row, with its name, for the "?"'s key.
    icons: Vec<(Icon, String)>,
}

fn bar_id() -> egui::Id {
    egui::Id::new("pane toolbar")
}

fn bar(ui: &Ui) -> Option<Bar> {
    ui.ctx().data(|d| d.get_temp::<Bar>(bar_id()))
}

fn set_bar(ui: &Ui, b: Option<Bar>) {
    ui.ctx().data_mut(|d| match b {
        Some(b) => d.insert_temp(bar_id(), b),
        None => d.remove::<Bar>(bar_id()),
    });
}

/// The widths of a toolbar's controls, last measured, and their order.
fn widths_id(title: &str) -> egui::Id {
    egui::Id::new(("pane toolbar widths", title))
}

/// What the toolbars drew in the latest pass that drew one, as
/// "Pane/Control", or "Pane/…" for a "…" menu, each with where: for the
/// tests, which can't see hover text.
#[cfg(test)]
pub fn tools_drawn_at(ctx: &egui::Context) -> Vec<(String, egui::Rect)> {
    ctx.data(|d| d.get_temp::<(u64, Vec<(String, egui::Rect)>)>(egui::Id::new("tools drawn")))
        .map(|(_, v)| v)
        .unwrap_or_default()
}

/// The names of what [`tools_drawn_at`] lists.
#[cfg(test)]
pub fn tools_drawn(ctx: &egui::Context) -> Vec<String> {
    tools_drawn_at(ctx).into_iter().map(|(k, _)| k).collect()
}

fn note_drawn(ui: &Ui, title: &str, key: &str, rect: egui::Rect) {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| {
        let e = d.get_temp_mut_or_default::<(u64, Vec<(String, egui::Rect)>)>(egui::Id::new(
            "tools drawn",
        ));
        if e.0 != pass {
            *e = (pass, Vec::new());
        }
        e.1.push((format!("{title}/{key}"), rect));
    });
}

/// Space between a toolbar's controls.
const BAR_GAP: f32 = 8.0;
/// The "…" button's width, kept free when anything folds.
const MORE_W: f32 = 30.0;

/// The keys of `order` that fold so the rest, with "…", fit in `room`:
/// the last ones first, as a Mac toolbar folds from its right, never
/// those in `keep`. A gap left at the end folds with what follows it.
fn fold(order: &[(String, f32)], keep: &[String], room: f32) -> Vec<String> {
    let kept = |k: &str| keep.iter().any(|c| c == k);
    // The row with the first `n` controls and every kept one after them.
    let shown = |n: usize| -> Vec<&(String, f32)> {
        order
            .iter()
            .enumerate()
            .filter(|&(i, (k, _))| i < n || kept(k))
            .map(|(_, c)| c)
            .collect()
    };
    let width = |n: usize| {
        let row = shown(n);
        row.iter().map(|(_, w)| w).sum::<f32>() + BAR_GAP * row.len().saturating_sub(1) as f32
    };
    if width(order.len()) <= room {
        return Vec::new();
    }
    let mut n = order.len();
    while n > 0 && width(n) + BAR_GAP + MORE_W > room {
        n -= 1;
    }
    while n > 0 && order[n - 1].0.starts_with('|') {
        n -= 1;
    }
    order[n..]
        .iter()
        .filter(|(k, _)| !kept(k))
        .map(|(k, _)| k.clone())
        .collect()
}

/// The next control in a pane's toolbar never folds into "…": the
/// pane's main action, such as New Watch… or Start Test.
pub fn keep_next(ui: &Ui) {
    if let Some(mut b) = bar(ui).filter(|b| b.depth == 0) {
        b.keep_next = true;
        set_bar(ui, Some(b));
    }
}

fn keep_id(title: &str) -> egui::Id {
    egui::Id::new(("pane toolbar keeps", title))
}

fn key_id(title: &str) -> egui::Id {
    egui::Id::new(("pane toolbar key", title))
}

/// Note a toolbar icon and its name for the toolbar's key, once, while
/// the row is drawn.
pub fn note_icon(ui: &Ui, icon: Icon, name: &str) {
    if let Some(mut b) = bar(ui).filter(|b| b.depth == 0 && b.menu.is_none()) {
        if !b.icons.iter().any(|(_, n)| n == name) {
            b.icons.push((icon, name.to_string()));
            set_bar(ui, Some(b));
        }
    }
}

/// A pane's toolbar: its controls in one row, its "?" at the right. What
/// doesn't fit folds into a "…" menu at the row's end, with words, and
/// comes back out when the pane is wide enough again; a pane's plot never
/// gives up room for its tools.
pub fn pane_toolbar(ui: &mut Ui, title: &str, help_text: &[&str], controls: impl FnMut(&mut Ui)) {
    pane_toolbar_full(ui, title, help_text, None, None, controls)
}

/// A pane's toolbar with `note`, a fact about what the pane shows (such
/// as the moment it is from), at its right end before the "?".
pub fn pane_toolbar_noted(
    ui: &mut Ui,
    title: &str,
    help_text: &[&str],
    note: Option<&str>,
    controls: impl FnMut(&mut Ui),
) {
    pane_toolbar_full(ui, title, help_text, note, None, controls)
}

/// [`pane_toolbar`] with one control `pinned` just left of the "?": it
/// never folds and never moves, however the row's controls fold.
pub fn pane_toolbar_pinned(
    ui: &mut Ui,
    title: &str,
    help_text: &[&str],
    pinned: Option<&mut dyn FnMut(&mut Ui)>,
    controls: impl FnMut(&mut Ui),
) {
    pane_toolbar_full(ui, title, help_text, None, pinned, controls)
}

/// A pane's toolbar: its controls, then at its right end its `note`, its
/// `pinned` control and the "?".
fn pane_toolbar_full(
    ui: &mut Ui,
    title: &str,
    help_text: &[&str],
    note: Option<&str>,
    pinned: Option<&mut dyn FnMut(&mut Ui)>,
    mut controls: impl FnMut(&mut Ui),
) {
    let full = ui.available_rect_before_wrap();
    let q = egui::Rect::from_min_size(
        egui::pos2(full.right() - 18.0, full.top() + 4.0),
        Vec2::splat(16.0),
    );
    let pin_w = if pinned.is_some() {
        CONTROL_H + 6.0
    } else {
        0.0
    };
    if let Some(pin) = pinned {
        let r = egui::Rect::from_min_size(
            egui::pos2(full.right() - 26.0 - CONTROL_H - 2.0, full.top() + 1.0),
            Vec2::new(CONTROL_H, CONTROL_H),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| pin(ui));
    }
    let note_w = note.map_or(0.0, |n| {
        let p = pal(ui);
        let g = ui.painter().layout_no_wrap(
            n.to_owned(),
            egui::FontId::proportional(12.0),
            p.text_secondary,
        );
        let w = g.size().x;
        ui.painter().galley(
            egui::pos2(
                full.right() - 26.0 - pin_w - w,
                q.center().y - g.size().y / 2.0,
            ),
            g,
            p.text_secondary,
        );
        note_drawn(
            ui,
            title,
            n,
            egui::Rect::from_min_size(
                egui::pos2(full.right() - 26.0 - pin_w - w, q.top()),
                Vec2::new(w, q.height()),
            ),
        );
        w + 12.0
    });
    let mut text = help_text.to_vec();
    text.push(crate::help::TOOLBAR);
    // The key from the row as last drawn.
    let key: Vec<(Icon, String)> = ui
        .ctx()
        .data(|d| d.get_temp(key_id(title)))
        .unwrap_or_default();
    ui.scope_builder(egui::UiBuilder::new().max_rect(q), |ui| {
        help_with_key(ui, title, &text, &key)
    });
    let room = full.with_max_x(full.right() - 26.0 - pin_w - note_w);
    // Last frame's controls and their widths decide what folds now.
    let known: Vec<(String, f32)> = ui
        .ctx()
        .data(|d| d.get_temp(widths_id(title)))
        .unwrap_or_default();
    let keep: Vec<String> = ui
        .ctx()
        .data(|d| d.get_temp(keep_id(title)))
        .unwrap_or_default();
    let folded = fold(&known, &keep, room.width());
    let mut kept = Vec::new();
    let mut icons = Vec::new();
    let outer = bar(ui);
    let mut widths: Vec<(String, f32)> = Vec::new();
    ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
        ui.set_clip_rect(room.intersect(ui.clip_rect()));
        ui.horizontal(|ui| {
            // The row as tall as its tallest field from the start, so each
            // control is centred on the same line.
            ui.set_min_height(CONTROL_H);
            ui.spacing_mut().item_spacing = Vec2::new(BAR_GAP, 4.0);
            set_bar(
                ui,
                Some(Bar {
                    title: title.to_string(),
                    folded: folded.clone(),
                    ..Default::default()
                }),
            );
            controls(ui);
            let row = bar(ui).unwrap_or_default();
            kept = row.keep.clone();
            icons = row.icons.clone();
            // Each control's width: as drawn now, or as last measured
            // while it is folded.
            let drawn: Vec<(String, f32)> = ui
                .ctx()
                .data(|d| d.get_temp(egui::Id::new(("pane toolbar drawn", title))))
                .unwrap_or_default();
            for k in &row.met {
                let w = drawn
                    .iter()
                    .chain(known.iter())
                    .find(|(d, _)| d == k)
                    .map_or(26.0, |(_, w)| *w);
                widths.push((k.clone(), w));
            }
            ui.ctx().data_mut(|d| {
                d.remove::<Vec<(String, f32)>>(egui::Id::new(("pane toolbar drawn", title)))
            });
            if !folded.is_empty() {
                let more = ui
                    .menu_button(RichText::new("…").size(14.0), |ui| {
                        ui.set_min_width(280.0);
                        ui.spacing_mut().item_spacing.y = 6.0;
                        set_bar(
                            ui,
                            Some(Bar {
                                title: title.to_string(),
                                menu: Some(()),
                                folded: folded.clone(),
                                ..Default::default()
                            }),
                        );
                        controls(ui);
                    })
                    .response
                    .on_hover_text("The settings the pane is too narrow to show here");
                note_drawn(ui, title, "…", more.rect);
            }
            set_bar(ui, outer.clone());
        });
    });
    ui.ctx().data_mut(|d| {
        d.insert_temp(widths_id(title), widths);
        d.insert_temp(keep_id(title), kept);
        d.insert_temp(key_id(title), icons);
    });
}

/// One control of a pane's toolbar, named `key`: drawn in the row while
/// it fits, else under "…", where `add` sees it isn't in the toolbar and
/// draws its settings-list form, with words. Anywhere else (a settings
/// window) it is drawn as it is. `None` when not drawn here.
pub fn bar_item<R>(ui: &mut Ui, key: &str, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let Some(mut b) = bar(ui).filter(|b| b.depth == 0) else {
        return Some(add(ui));
    };
    b.met.push(key.to_string());
    if std::mem::take(&mut b.keep_next) && b.menu.is_none() {
        b.keep.push(key.to_string());
    }
    let folded = b.folded.iter().any(|k| k == key);
    let show = folded == b.menu.is_some();
    b.depth = 1;
    set_bar(ui, Some(b.clone()));
    let out = if !show {
        None
    } else if b.menu.is_some() {
        Some(add(ui))
    } else {
        let inner = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            add(ui)
        });
        let w = inner.response.rect.width();
        let id = egui::Id::new(("pane toolbar drawn", b.title.as_str()));
        ui.ctx().data_mut(|d| {
            d.get_temp_mut_or_default::<Vec<(String, f32)>>(id)
                .push((key.to_string(), w));
        });
        note_drawn(ui, &b.title, key, inner.response.rect);
        Some(inner.inner)
    };
    b.depth = 0;
    set_bar(ui, Some(b));
    out
}

/// A thin line between groups of a toolbar's controls; folded with what
/// follows it.
pub fn bar_gap(ui: &mut Ui) {
    let n = bar(ui).map_or(0, |b| b.met.len());
    bar_item(ui, &format!("|{n}"), |ui| {
        if in_toolbar(ui) {
            // As tall as an icon, not the whole pane.
            let (rect, _) = ui.allocate_exact_size(Vec2::new(6.0, 18.0), Sense::hover());
            ui.painter().vline(
                rect.center().x,
                rect.y_range(),
                Stroke::new(1.0_f32, pal(ui).separator),
            );
        } else {
            ui.separator();
        }
    });
}

/// The words a toolbar icon shows on hover: its name, then what it does.
fn hover(label: &str, hint: &str) -> String {
    if hint.is_empty() {
        label.to_string()
    } else {
        format!("{label}: {hint}")
    }
}

/// A switch in a toolbar: its icon, tinted in `color` while on; with
/// words under "…" or in a settings list. True when flipped.
pub fn tool_switch(
    ui: &mut Ui,
    icon: Icon,
    color: Color32,
    label: &str,
    on: &mut bool,
    hint: &str,
) -> bool {
    note_icon(ui, icon, label);
    bar_item(ui, label, |ui| {
        if in_toolbar(ui) {
            let r = icon_toggle(ui, icon, *on, color).on_hover_text(hover(label, hint));
            if r.clicked() {
                *on = !*on;
            }
            r.clicked()
        } else {
            switch_row(ui, label, on, hint)
        }
    })
    .unwrap_or(false)
}

/// A button in a toolbar: its icon, or its words under "…". `disabled`
/// says why it can't be used now, when it can't.
pub fn tool_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    hint: &str,
    disabled: Option<&str>,
) -> bool {
    note_icon(ui, icon, label);
    bar_item(ui, label, |ui| {
        let r = ui
            .add_enabled_ui(disabled.is_none(), |ui| {
                if in_toolbar(ui) {
                    icon_tool(ui, icon)
                } else {
                    ui.button(label)
                }
            })
            .inner;
        r.on_hover_text(hover(label, hint))
            .on_disabled_hover_text(hover(label, disabled.unwrap_or("")))
            .clicked()
    })
    .unwrap_or(false)
}

/// A button that keeps its words beside its icon in a toolbar, such as
/// Start Test: filled in the accent colour when `primary`.
pub fn tool_word_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    hint: &str,
    primary: bool,
    disabled: Option<&str>,
) -> bool {
    note_icon(ui, icon, label);
    bar_item(ui, label, |ui| {
        let p = pal(ui);
        let r = ui
            .add_enabled_ui(disabled.is_none(), |ui| {
                let enabled = ui.is_enabled();
                let ink = if primary && enabled {
                    p.on_accent
                } else if enabled {
                    p.text
                } else {
                    p.text_tertiary
                };
                let size = if in_toolbar(ui) { 12.5 } else { 13.0 };
                let font = if primary {
                    semibold(size)
                } else {
                    FontId::proportional(size)
                };
                let galley = ui.painter().layout_no_wrap(label.to_string(), font, ink);
                let icon_w = 14.0;
                let pad = Vec2::new(8.0, 3.0);
                let size = Vec2::new(
                    pad.x * 2.0 + icon_w + 5.0 + galley.size().x,
                    (galley.size().y + pad.y * 2.0).max(24.0),
                );
                let sense = if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let (rect, resp) = ui.allocate_exact_size(size, sense);
                let fill = if primary && enabled {
                    p.accent
                } else if resp.hovered() && enabled {
                    p.control_hover
                } else {
                    p.control
                };
                let radius = CornerRadius::same(CONTROL_RADIUS);
                ui.painter().rect_filled(rect, radius, fill);
                let ir = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + pad.x + icon_w / 2.0, rect.center().y),
                    Vec2::splat(icon_w),
                );
                crate::icons::draw(ui.painter(), ir, icon, ink);
                ui.painter().galley(
                    egui::pos2(ir.right() + 5.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    ink,
                );
                resp
            })
            .inner;
        r.on_hover_text(hint)
            .on_disabled_hover_text(disabled.unwrap_or(""))
            .clicked()
    })
    .unwrap_or(false)
}

/// A toolbar control that holds a value, such as a field or a menu: its
/// name in words, the control after it; with its name under "…" or in a
/// settings list. Icons are kept for what can be clicked, so a name that
/// only labels a control stays a word.
pub fn tool_field<R>(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    control: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    bar_item(ui, label, |ui| {
        if in_toolbar(ui) {
            let p = pal(ui);
            ui.add(
                egui::Label::new(RichText::new(label).small().color(p.text_secondary))
                    .selectable(false),
            )
            .on_hover_text(hover(label, hint));
            control(ui)
        } else {
            row(ui, label, Some(hint), control)
        }
    })
}

/// A search box in a toolbar: the magnifying glass every app puts beside
/// one, then the box; with its name under "…" or in a settings list.
pub fn tool_search<R>(ui: &mut Ui, hint: &str, control: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    note_icon(ui, Icon::Search, "Search");
    bar_item(ui, "Search", |ui| {
        if in_toolbar(ui) {
            let p = pal(ui);
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            crate::icons::draw(ui.painter(), rect, Icon::Search, p.text_secondary);
            resp.on_hover_text(hover("Search", hint));
            control(ui)
        } else {
            row(ui, "Search", Some(hint), control)
        }
    })
}

/// One of a few choices in a toolbar: a row of icons, the picked one
/// tinted; under "…" or in a settings list, the choices in words. True
/// when the choice changed. Two choices are one button, not two icons:
/// the icon of the choice in use, and a click changes to the other.
pub fn tool_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    value: &mut T,
    choices: &[(T, Icon, &str, &str)],
) -> bool {
    if let [(off, off_icon, off_name, off_why), (on, on_icon, on_name, on_why)] = *choices {
        note_icon(ui, off_icon, &format!("{label}: {off_name}"));
        note_icon(ui, on_icon, &format!("{label}: {on_name}"));
        return bar_item(ui, label, |ui| {
            if in_toolbar(ui) {
                // Two modes, not on and off: the icon is the mode in use,
                // and a click changes to the other.
                let now_on = *value == on;
                let (icon, name, why, other) = if now_on {
                    (on_icon, on_name, on_why, off_name)
                } else {
                    (off_icon, off_name, off_why, on_name)
                };
                let r = icon_tool(ui, icon)
                    .on_hover_text(format!("{label}: {}. Click for {other}.", hover(name, why)));
                if r.clicked() {
                    *value = if now_on { off } else { on };
                }
                r.clicked()
            } else {
                let words = [(off, off_name, off_why), (on, on_name, on_why)];
                row(ui, label, Some(hint), |ui| segmented(ui, value, &words))
            }
        })
        .unwrap_or(false);
    }
    for &(_, icon, name, _) in choices {
        note_icon(ui, icon, &format!("{label}: {name}"));
    }
    bar_item(ui, label, |ui| {
        if in_toolbar(ui) {
            let p = pal(ui);
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut changed = false;
            for &(v, icon, name, why) in choices {
                let picked = *value == v;
                let r = icon_toggle(ui, icon, picked, p.accent)
                    .on_hover_text(format!("{label}: {}", hover(name, why)));
                if r.clicked() && !picked {
                    *value = v;
                    changed = true;
                }
            }
            changed
        } else {
            let words: Vec<(T, &str, &str)> =
                choices.iter().map(|&(v, _, n, h)| (v, n, h)).collect();
            row(ui, label, Some(hint), |ui| segmented(ui, value, &words))
        }
    })
    .unwrap_or(false)
}

/// A square button with only an icon, for a toolbar: plain until hovered.
pub fn icon_tool(ui: &mut Ui, icon: Icon) -> Response {
    let p = pal(ui);
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), sense);
    if resp.hovered() && ui.is_enabled() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), p.control_hover);
    }
    let ink = if ui.is_enabled() {
        p.text_secondary
    } else {
        p.text_tertiary.gamma_multiply(0.6)
    };
    crate::icons::draw(ui.painter(), rect.shrink(4.0), icon, ink);
    resp
}

/// A button that stays pressed while `on`: a switch in a toolbar. Its
/// response is marked changed when clicked.
pub fn toggle(ui: &mut Ui, label: &str, on: &mut bool) -> Response {
    let p = pal(ui);
    let text = RichText::new(label)
        .size(12.5)
        .color(if *on { p.text } else { p.text_secondary });
    // One line always: a narrow toolbar must not stack the label's letters.
    let mut resp = ui.add(
        egui::Button::new(text)
            .wrap_mode(egui::TextWrapMode::Extend)
            .selected(*on)
            .corner_radius(CornerRadius::same(CONTROL_RADIUS)),
    );
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp
}

/// The small pictures on the pane buttons at the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Watch,
    Readings,
    Strip,
    Profile,
    Scope,
    Steadiness,
    Periods,
    Cycle,
    Autocorrelation,
    Allan,
    Cusum,
    Fft,
    Stft,
    Wavelet,
    Reserve,
    Microphone,
    Isochronism,
    Positions,
    /// The readings themselves: a hare, a swing and a seesaw, wherever
    /// the app shows or toggles one of them.
    Rate,
    Amplitude,
    BeatError,
    Charts,
    Distributions,
    Settings,
    Listen,
    Stop,
    Save,
    DialUp,
    DialDown,
    CrownUp,
    CrownDown,
    CrownLeft,
    CrownRight,
    Export,
    Bucket,
    Log,
    Copy,
    SaveTable,
    Calipers,
    Tape,
    RateLine,
    Rails,
    Truing,
    AutoCentre,
    Masks,
    ArrowDown,
    ArrowRight,
    Stacked,
    Beside,
    Db,
    PalletFork,
    OneClock,
    TwoClocks,
    Notes,
    Trace,
    Drum,
    Traces,
    Stopwatch,
    Gears,
    Roll,
    ProbLine,
    SCurve,
    TwoSeconds,
    BarrelIn,
    Barrel24,
    Hourglass,
    Wave,
    Sheet,
    Search,
    Sort,
    WatchBox,
    NewWatch,
    Back,
    Screwdriver,
    NewPhoto,
    Funnel,
    Fit,
    Highlighter,
    Info,
}

/// What an alert says about itself: a warning (a triangle in the text
/// colour) or a failure (Bad's red, on the icon only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Warning,
    Bad,
}

/// An alert's icon in `rect`: the words beside it stay in text colour.
pub fn alert_icon(painter: &egui::Painter, rect: egui::Rect, kind: Alert, p: &Palette) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) / 2.0;
    let (ink, mark) = match kind {
        Alert::Warning => {
            let top = c + Vec2::new(0.0, -r * 0.95);
            let left = c + Vec2::new(-r, r * 0.8);
            let right = c + Vec2::new(r, r * 0.8);
            painter.add(egui::Shape::closed_line(
                vec![top, right, left],
                Stroke::new(1.3_f32, p.text),
            ));
            (p.text, c + Vec2::new(0.0, r * 0.12))
        }
        Alert::Bad => {
            painter.circle_filled(c, r, p.bad);
            (p.on_accent, c)
        }
    };
    let stem = r * 0.36;
    painter.line_segment(
        [
            mark + Vec2::new(0.0, -stem),
            mark + Vec2::new(0.0, stem * 0.45),
        ],
        Stroke::new(1.4_f32, ink),
    );
    painter.circle_filled(mark + Vec2::new(0.0, stem * 1.05), 0.85, ink);
}

/// An alert's icon, then its words in the text colour (`secondary` for a
/// caption).
pub fn alert_label(ui: &mut Ui, kind: Alert, words: &str, small: bool) -> Response {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let size = if small { 12.0 } else { 14.0 };
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        alert_icon(ui.painter(), rect, kind, p);
        let text = RichText::new(words).color(if small { p.text_secondary } else { p.text });
        ui.add(egui::Label::new(if small { text.small() } else { text }).wrap());
    })
    .response
}

/// Diagonal hatching across `rect` (lines at 45°, `pitch` apart): how a
/// masked stretch is drawn, so it can't be taken for a faint one.
pub fn hatch(painter: &egui::Painter, rect: egui::Rect, color: Color32, width: f32, pitch: f32) {
    if !rect.is_positive() {
        return;
    }
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let h = rect.height();
    // Lines anchored to whole multiples of the pitch, so neighbouring
    // rects' hatching lines up.
    let mut x = ((rect.left() - h) / pitch).floor() * pitch;
    while x <= rect.right() {
        painter.line_segment(
            [egui::pos2(x, rect.bottom()), egui::pos2(x + h, rect.top())],
            Stroke::new(width, color),
        );
        x += pitch;
    }
}

/// A square button with an icon. While `on` it is a chip tinted in `color`
/// with the icon in full colour; while off the icon is a faint grey, so the
/// panes that show stand out at a glance.
pub fn icon_toggle(ui: &mut Ui, icon: Icon, on: bool, color: Color32) -> Response {
    let off = pal(ui).text_tertiary.gamma_multiply(0.75);
    icon_toggle_off(ui, icon, on, color, off)
}

/// [`icon_toggle`], with the icon in `off` while off: the pane buttons'
/// icons are the secondary text colour, and the accent while shown.
pub fn icon_toggle_off(
    ui: &mut Ui,
    icon: Icon,
    on: bool,
    color: Color32,
    off: Color32,
) -> Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    let radius = CornerRadius::same(CONTROL_RADIUS);
    if on {
        ui.painter().rect(
            rect.shrink(0.5),
            radius,
            color.gamma_multiply(0.16),
            Stroke::new(1.0_f32, color.gamma_multiply(0.45)),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, radius, p.control_hover);
    }
    let c = if on { color } else { off };
    crate::icons::draw(ui.painter(), rect.shrink(4.0), icon, c);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), on, "")
    });
    resp
}

/// A square button with an icon, `size` across, for a control on a card:
/// filled in `color` when `strong`, else a plain button. Picked, it is
/// tinted as a toggle that is on. Its corners are `radius`.
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    size: f32,
    radius: u8,
    color: Color32,
    strong: bool,
    picked: bool,
) -> Response {
    let p = pal(ui);
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), sense);
    let radius = CornerRadius::same(radius);
    let enabled = ui.is_enabled();
    let (fill, ink) = if strong && enabled {
        (color, Color32::WHITE)
    } else if picked {
        (color.gamma_multiply(0.18), color)
    } else if resp.hovered() && enabled {
        (p.control_hover, p.text)
    } else {
        (p.control, if enabled { p.text } else { p.text_tertiary })
    };
    ui.painter().rect_filled(rect, radius, fill);
    if picked {
        ui.painter().rect_stroke(
            rect.shrink(0.5),
            radius,
            Stroke::new(1.0_f32, color.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }
    crate::icons::draw(ui.painter(), rect.shrink(size * 0.2), icon, ink);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, picked, ""));
    resp
}
