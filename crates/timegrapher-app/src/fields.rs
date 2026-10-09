//! Typed number entries: a text box you type a value into, committed with
//! Enter or by clicking away, with a short list of common values beside it.

use eframe::egui;

/// How a field shows and reads its value.
pub struct Format {
    pub show: fn(f64) -> String,
    pub parse: fn(&str) -> Option<f64>,
}

/// Plain number with up to one decimal, and an optional unit after it.
pub fn plain(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

/// Read a number, allowing a decimal comma and a trailing unit such as
/// "ms", "s" or "°".
pub fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let s = s.trim_start_matches('±');
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(s.len());
    let v: f64 = s[..end].parse().ok()?;
    v.is_finite().then_some(v)
}

/// Seconds shown as "30 s", "5 min", "2 h" or "1:30:00".
pub fn duration(v: f64) -> String {
    let s = v.round().max(0.0) as u64;
    if s < 120 {
        format!("{} s", plain(v))
    } else if s.is_multiple_of(3600) {
        format!("{} h", s / 3600)
    } else if s.is_multiple_of(60) && s < 7200 {
        format!("{} min", s / 60)
    } else {
        crate::strip::fmt_time(v)
    }
}

/// Read "45", "45 s", "5 min", "5m", "2 h", "1:30" or "1:02:30" as seconds.
pub fn parse_duration(s: &str) -> Option<f64> {
    let s = s.trim().to_lowercase().replace(',', ".");
    if s.contains(':') {
        let mut total = 0.0;
        for part in s.split(':') {
            let v: f64 = part.trim().parse().ok()?;
            total = total * 60.0 + v;
        }
        return Some(total);
    }
    let v = parse_number(&s)?;
    let unit = s.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ');
    let k = match unit.trim() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
        _ => return None,
    };
    Some(v * k)
}

/// A text box for `value`. While it has focus the typed text is kept as is;
/// Enter or clicking away reads it, clamps it to `range` and returns true if
/// the value changed. Text that doesn't read as a number is dropped.
pub fn entry(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    fmt: &Format,
    width: f32,
) -> bool {
    let id = ui.make_persistent_id(id_salt);
    let buf = id.with("text");
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(buf))
        .unwrap_or_else(|| (fmt.show)(*value));
    let resp = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(id)
            .desired_width(width),
    );
    let mut changed = false;
    if resp.lost_focus() {
        if let Some(v) = (fmt.parse)(&text) {
            let v = v.clamp(*range.start(), *range.end());
            if v != *value {
                *value = v;
                changed = true;
            }
        }
        ui.data_mut(|d| d.remove::<String>(buf));
    } else if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(buf, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(buf));
    }
    changed
}

/// A drop-down of common values next to an entry. Returns true if one was
/// picked.
pub fn presets(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: &mut f64,
    options: &[f64],
    show: fn(f64) -> String,
) -> bool {
    let mut picked = *value;
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text("")
        .width(18.0)
        .show_ui(ui, |ui| {
            for &o in options {
                ui.selectable_value(&mut picked, o, show(o));
            }
        });
    if picked != *value {
        *value = picked;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_numbers_and_durations() {
        assert_eq!(parse_number("52"), Some(52.0));
        assert_eq!(parse_number(" 52,5° "), Some(52.5));
        assert_eq!(parse_number("±10 ms"), Some(10.0));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_duration("45"), Some(45.0));
        assert_eq!(parse_duration("5 min"), Some(300.0));
        assert_eq!(parse_duration("5m"), Some(300.0));
        assert_eq!(parse_duration("2h"), Some(7200.0));
        assert_eq!(parse_duration("1:30"), Some(90.0));
        assert_eq!(parse_duration("1:02:30"), Some(3750.0));
        assert_eq!(parse_duration("5 fortnights"), None);
        assert_eq!(duration(30.0), "30 s");
        assert_eq!(duration(300.0), "5 min");
        assert_eq!(duration(7200.0), "2 h");
        assert_eq!(duration(3750.0), "1:02:30");
        assert_eq!(plain(62.5), "62.5");
        assert_eq!(plain(10.0), "10");
    }
}
