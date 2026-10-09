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

/// The colours of one theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Behind everything: the window, the sidebar, the gaps between cards.
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
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    /// The marks on the tick tock profile.
    pub unlock: Color32,
    pub drop: Color32,
    pub peak: Color32,
    /// The three sounds on the tick tock profile.
    pub sound: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
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
    on_accent: Color32::WHITE,
    tick: rgb(0x64d2ff),
    tock: rgb(0xff9f0a),
    good: rgb(0x30d158),
    warn: rgb(0xffb340),
    bad: rgb(0xff453a),
    unlock: rgb(0x30d158),
    drop: rgb(0xff453a),
    peak: rgb(0xbf5af2),
    sound: rgb(0xffd60a),
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
    on_accent: Color32::WHITE,
    tick: rgb(0x0068c9),
    tock: rgb(0xc45100),
    good: rgb(0x248a3d),
    warn: rgb(0xb25000),
    bad: rgb(0xd70015),
    unlock: rgb(0x248a3d),
    drop: rgb(0xd70015),
    peak: rgb(0x8944ab),
    sound: rgb(0x8a6d00),
};

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

/// Paint the card colour behind `contents` and lay them out inside.
pub fn card_ui<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let fill = pal(ui).card;
    card()
        .fill(fill)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui)
        })
        .inner
}

/// A small, spaced, upper-case caption over a group of settings or a
/// reading.
pub fn caption(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .font(semibold(11.0))
        .extra_letter_spacing(0.6)
}

/// A small round "?" that opens an explanation beside it, closed by
/// clicking anywhere else. The one way the app explains itself at length;
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
    let resp = resp.on_hover_text(format!("What {} means", title.to_lowercase()));
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(360.0)
        .show(|ui| {
            ui.set_max_width(360.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new(title).font(semibold(14.0)));
            for para in paragraphs {
                ui.add(egui::Label::new(RichText::new(*para).size(12.5)).wrap());
            }
        });
    resp
}

/// The caption over a card of settings, with its explanation and, for a
/// pane, the switch that shows or hides it. True when the switch flipped.
pub fn section_header(
    ui: &mut Ui,
    text: &str,
    shown: Option<&mut bool>,
    help_text: &[&str],
) -> bool {
    let mut changed = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(caption(text).color(pal(ui).text_secondary));
        if !help_text.is_empty() {
            help(ui, text, help_text);
        }
        if let Some(on) = shown {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(14.0);
                let hint = if *on {
                    format!("Hide the {}", text.to_lowercase())
                } else {
                    format!("Show the {}", text.to_lowercase())
                };
                changed = switch(ui, on).on_hover_text(hint).changed();
            });
        }
    });
    ui.add_space(2.0);
    changed
}

/// A row in a settings card: the name on the left (with its hint on hover)
/// and the control after it, lined up with the rows above and below.
pub fn row<R>(
    ui: &mut Ui,
    label: &str,
    hint: Option<&str>,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
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
/// True when the choice changed.
pub fn segmented<T: PartialEq + Copy>(
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
                for &(v, label, hint) in choices {
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
                    if !hint.is_empty() {
                        r = r.on_hover_text(hint);
                    }
                    if r.clicked() && !selected {
                        *value = v;
                        changed = true;
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

/// A coloured dot, the key to a line or side.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
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
