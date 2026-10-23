//! The look of the app: colours for the light and dark themes, the type
//! scale, spacing, and a few widgets that egui doesn't have (cards, section
//! headers, a segmented control, switches and chips).
//!
//! Colour carries meaning and nothing else: Tick is blue and Tock orange
//! everywhere, red, amber and green are for the state of the input, and one
//! accent marks what can be pressed or is selected. The rest is greys.

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
    /// Text on the accent colour (and on red).
    pub on_accent: Color32,
    pub tick: Color32,
    pub tock: Color32,
    pub warn: Color32,
    pub bad: Color32,
    /// The marks on the tick tock profile.
    pub unlock: Color32,
    pub drop: Color32,
    pub peak: Color32,
    /// The three sounds on the tick tock profile.
    pub sound: Color32,
    /// A calibre's wheels, where their turns are marked on a period axis.
    pub wheel: Color32,
    /// Amplitude, rate and beat error wherever they are drawn: over the
    /// strip, on their charts, in their distributions and in Steadiness.
    /// Each colour means one thing across the app, so the profile's marks
    /// and sounds use colours none of the readings do.
    pub trace_amplitude: Color32,
    pub trace_rate: Color32,
    pub trace_beat_error: Color32,
    /// Two colours only the pane buttons use: the Paper Strip's dancer and
    /// the Positions Test's die.
    pub pink: Color32,
    pub teal: Color32,
}

const fn c(x: core_palette::Rgb) -> Color32 {
    Color32::from_rgb(x.r, x.g, x.b)
}

/// The core palette (shared with the MCP server's charts) in egui's colours.
const fn from_core(p: &core_palette::Palette, pink: Color32, teal: Color32) -> Palette {
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
        warn: c(p.warn),
        bad: c(p.bad),
        unlock: c(p.unlock),
        drop: c(p.drop),
        peak: c(p.peak),
        sound: c(p.sound),
        wheel: c(p.wheel),
        trace_amplitude: c(p.trace_amplitude),
        trace_rate: c(p.trace_rate),
        trace_beat_error: c(p.trace_beat_error),
        pink,
        teal,
    }
}

pub const DARK: Palette = from_core(
    &core_palette::DARK,
    Color32::from_rgb(0xff, 0x37, 0x5f),
    Color32::from_rgb(0x40, 0xc8, 0xe0),
);
pub const LIGHT: Palette = from_core(
    &core_palette::LIGHT,
    Color32::from_rgb(0xd3, 0x0f, 0x45),
    Color32::from_rgb(0x0e, 0x7c, 0x86),
);

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
pub const CARD_PADDING: i8 = 14;

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
    v.warn_fg_color = p.warn;
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
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(360.0)
        .show(|ui| body(ui, true));
    resp
}

/// The caption over a card of settings, with its explanation and, for a
/// pane, the switch that shows or hides it. True when the switch flipped.
pub fn section_header(
    ui: &mut Ui,
    text: &str,
    shown: Option<&mut bool>,
    help_text: &[&str],
    folded: &mut bool,
) -> bool {
    let mut changed = false;
    let p = pal(ui);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // The chevron and the name fold the card away or open it again.
        let galley =
            ui.painter()
                .layout_no_wrap(text.to_string(), semibold(12.5), p.text_secondary);
        let size = Vec2::new(18.0 + galley.size().x, 20.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let color = if resp.hovered() {
            p.text
        } else {
            p.text_secondary
        };
        let c = egui::pos2(rect.left() + 8.0, rect.center().y);
        let t = ui.ctx().animate_bool_responsive(resp.id, !*folded);
        // A triangle pointing right when folded, turning to point down.
        let angle = t * std::f32::consts::FRAC_PI_2;
        let rot = |x: f32, y: f32| {
            let (s, co) = angle.sin_cos();
            c + Vec2::new(x * co - y * s, x * s + y * co)
        };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![rot(3.5, 0.0), rot(-2.5, 3.5), rot(-2.5, -3.5)],
            color,
            Stroke::NONE,
        ));
        ui.painter().galley_with_override_text_color(
            egui::pos2(rect.left() + 18.0, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
        if resp.clicked() {
            *folded = !*folded;
        }
        resp.on_hover_text(if *folded {
            format!("Open the {} card", text)
        } else {
            format!("Fold the {} card away", text)
        })
        .on_hover_cursor(egui::CursorIcon::PointingHand);
        if !help_text.is_empty() {
            help(ui, text, help_text);
        }
        if let Some(on) = shown {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(14.0);
                let hint = if *on {
                    format!("Hide the {}", text)
                } else {
                    format!("Show the {}", text)
                };
                changed = switch(ui, on).on_hover_text(hint).changed();
            });
        }
    });
    ui.add_space(2.0);
    changed
}

/// Lays out `add` as one piece in a pane toolbar's wrapping row, starting
/// a new line first when the piece didn't fit on the last frame. egui only
/// wraps single widgets, so a group (a name and its control, or a
/// segmented control) would otherwise run off the pane's edge.
fn keep_together<R>(ui: &mut Ui, key: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.id().with(("toolbar piece", key));
    let last: Option<f32> = ui.ctx().data(|d| d.get_temp(id));
    if last.is_some_and(|w| w > ui.available_size_before_wrap().x)
        && ui.cursor().left() > ui.max_rect().left() + 1.0
    {
        ui.end_row();
    }
    let inner = ui.horizontal(add);
    let w = inner.response.rect.width();
    ui.ctx().data_mut(|d| d.insert_temp(id, w));
    inner.inner
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
        return keep_together(ui, label, |ui| {
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
        });
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
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    choices: &[(T, &str, &str)],
) -> bool {
    if in_toolbar(ui) {
        let key = choices.first().map_or("", |c| c.1);
        return keep_together(ui, key, |ui| segmented_body(ui, value, choices));
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

/// A filled red button, for stopping what is running.
pub fn destructive(ui: &Ui, text: &str) -> egui::Button<'static> {
    let p = pal(ui);
    egui::Button::new(RichText::new(text).font(semibold(13.0)).color(p.on_accent))
        .fill(p.bad)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .min_size(Vec2::new(84.0, 26.0))
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

/// Whether controls are being drawn into a pane's toolbar, where they sit
/// in a row rather than one above another.
pub fn in_toolbar(ui: &Ui) -> bool {
    ui.ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::new("pane toolbar")))
        .unwrap_or(false)
}

/// A pane's toolbar: its controls in a row that wraps when the pane is
/// narrow, and its "?" at the right.
pub fn pane_toolbar(ui: &mut Ui, title: &str, help_text: &[&str], controls: impl FnOnce(&mut Ui)) {
    let id = egui::Id::new("pane toolbar");
    let full = ui.available_rect_before_wrap();
    let q = egui::Rect::from_min_size(
        egui::pos2(full.right() - 18.0, full.top() + 4.0),
        Vec2::splat(16.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(q), |ui| {
        help(ui, title, help_text)
    });
    let room = full.with_max_x(full.right() - 26.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 4.0);
            ui.ctx().data_mut(|d| d.insert_temp(id, true));
            controls(ui);
            ui.ctx().data_mut(|d| d.insert_temp(id, false));
        });
    });
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
    RateChart,
    AmplitudeChart,
    BeatErrorChart,
    RateDistribution,
    AmplitudeDistribution,
    BeatErrorDistribution,
    Settings,
}

/// A square button with an icon. While `on` it is a chip tinted in `color`
/// with the icon in full colour; while off the icon is a faint grey, so the
/// panes that show stand out at a glance.
pub fn icon_toggle(ui: &mut Ui, icon: Icon, on: bool, color: Color32) -> Response {
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
    let c = if on {
        color
    } else {
        p.text_tertiary.gamma_multiply(0.75)
    };
    crate::icons::draw(ui.painter(), rect.shrink(4.0), icon, c);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), on, "")
    });
    resp
}
