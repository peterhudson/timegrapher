//! When a watch was last fully wound, and each take's hours since then
//! (#1665): the moment kept as UTC, a take's `wind_h` the hours from it to
//! the take's start, to the tenth every surface keys a state of wind to.

use crate::session::Manifest;
use crate::take;
use std::path::Path;

/// Unix seconds of a UTC time as [`crate::recorder::utc`] writes it
/// (`2026-10-08T21:04:05.123Z`, the fraction and `Z` optional), or none
/// for a time it can't read.
pub fn unix_of_utc(s: &str) -> Option<f64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date
        .splitn(3, '-')
        .map(|p| Some(p).filter(|p| digits(p))?.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.splitn(3, ':');
    let hour = t.next().filter(|p| digits(p))?.parse::<i64>().ok()?;
    let minute = t.next().filter(|p| digits(p))?.parse::<i64>().ok()?;
    let second = match t.next() {
        None => 0.0,
        Some(p) => {
            let (whole, frac) = p.split_once('.').unwrap_or((p, "0"));
            if !digits(whole) || !digits(frac) {
                return None;
            }
            p.parse::<f64>().ok()?
        }
    };
    // A real civil date and a time within its day: 2025-02-29, 24:00 and
    // 12:60 are refused, not rolled into the next day or hour (#2422).
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0.0..60.0).contains(&second) {
        return None;
    }
    let date = chrono::NaiveDate::from_ymd_opt(
        i32::try_from(year).ok()?,
        u32::try_from(month).ok()?,
        u32::try_from(day).ok()?,
    )?;
    let days = date
        .signed_duration_since(chrono::NaiveDate::from_ymd_opt(1970, 1, 1)?)
        .num_days();
    Some((days * 86400 + hour * 3600 + minute * 60) as f64 + second)
}

/// True when `p` is one or more ASCII digits, so no sign, space or
/// exponent slips past `parse`.
fn digits(p: &str) -> bool {
    !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())
}

/// Hours since full wind `h` in whole tenths of an hour, rounded: the
/// one step every state of wind is kept, keyed and said to (#1746).
pub fn tenths(h: f64) -> f64 {
    (h * 10.0).round()
}

/// Hours since full wind `h` to the nearest tenth ([`tenths`]).
pub fn to_tenth(h: f64) -> f64 {
    tenths(h) / 10.0
}

/// Hours from full wind at `wound_unix` to `at_unix`, to a tenth: never
/// less than full wind.
pub fn hours_since(wound_unix: f64, at_unix: f64) -> f64 {
    to_tenth(((at_unix - wound_unix) / 3600.0).max(0.0))
}

/// A take's hours since full wind at its start (`began_unix`), to a
/// tenth: none for a full wind after the take began, never clamped to
/// it (Norm on #1935: the readings before it were at some other wind,
/// unknown, so a clamp would fill one in).
pub fn wind_h_at_start(wound_unix: f64, began_unix: f64) -> Option<f64> {
    (wound_unix <= began_unix).then(|| hours_since(wound_unix, began_unix))
}

/// A take's hours since full wind, as its session file gives them: its
/// first entry's that has them, or none where the wind wasn't given.
pub fn take_wind_h(m: &Manifest) -> Option<f64> {
    m.recordings.iter().find_map(|r| r.wind_h)
}

/// Give the take `take` its hours since full wind, `wind_h` (none where
/// it isn't given), in every entry of its session file, its own wording
/// kept: Set Wind on a take in Watches.
pub fn set_take_wind_h(take: &Path, wind_h: Option<f64>) -> Result<(), String> {
    let file = take.join(take::MANIFEST);
    let text =
        crate::recorder::read_settled(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let entries = doc
        .get_mut("recording")
        .and_then(|r| r.as_array_of_tables_mut())
        .filter(|a| !a.is_empty())
        .ok_or_else(|| format!("{} lists no recordings", file.display()))?;
    for e in entries.iter_mut() {
        match wind_h {
            Some(h) => {
                e.insert("wind_h", toml_edit::value(to_tenth(h)));
            }
            None => {
                e.remove("wind_h");
            }
        }
    }
    crate::library::write_atomic(&file, doc.to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_reads_back_as_written() {
        for ms in [0u64, 951_782_400_000, 1_791_493_065_123, 4_102_444_799_999] {
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms);
            let back = unix_of_utc(&crate::recorder::utc(t)).unwrap();
            assert!((back - ms as f64 / 1000.0).abs() < 1e-6, "{ms}");
        }
        assert_eq!(unix_of_utc("2026-10-08T20:57"), Some(1_791_493_020.0));
        assert_eq!(unix_of_utc("not a time"), None);
        assert_eq!(unix_of_utc("2026-13-08T20:57:00Z"), None);
    }

    #[test]
    fn impossible_dates_and_times_are_refused_not_rolled_over() {
        // #2422: these used to roll into the next day, hour or minute.
        for bad in [
            "2025-02-29T12:00:00Z",
            "2026-04-31T12:00:00Z",
            "2026-11-25T24:00:00Z",
            "2026-11-25T12:60:00Z",
            "2026-11-25T12:00:60Z",
            "2026-11-25T12:00:60.5Z",
            "2026-11-25T-1:00:00Z",
            "2026-11-25T12:00:+5Z",
            "2026-11-25T12:00:1e1Z",
            "2026-11-25T12:00:NaNZ",
            "2026-00-10T12:00:00Z",
            "2026-11-25T12:00:.Z",
        ] {
            assert_eq!(unix_of_utc(bad), None, "{bad}");
        }
        // Real leap days and the documented optional forms still read.
        assert_eq!(unix_of_utc("2024-02-29T12:00:00Z"), Some(1_709_208_000.0));
        assert_eq!(unix_of_utc("2000-02-29T00:00:00Z"), Some(951_782_400.0));
        assert_eq!(
            unix_of_utc("2026-11-25T23:59:59.999Z"),
            Some(1_795_651_199.999)
        );
        assert_eq!(unix_of_utc("2026-11-25T23:59"), Some(1_795_651_140.0));
        assert_eq!(unix_of_utc("2026-11-25T23:59:59"), Some(1_795_651_199.0));
    }

    #[test]
    fn hours_since_full_wind_are_to_a_tenth_and_never_before_it() {
        assert_eq!(hours_since(0.0, 2.0 * 3600.0 + 170.0), 2.0);
        assert_eq!(hours_since(0.0, 24.24 * 3600.0), 24.2);
        assert_eq!(hours_since(100.0, 50.0), 0.0);
    }

    #[test]
    fn a_full_wind_after_the_take_began_gives_the_take_no_wind() {
        assert_eq!(wind_h_at_start(0.0, 2.0 * 3600.0 + 170.0), Some(2.0));
        assert_eq!(wind_h_at_start(100.0, 100.0), Some(0.0));
        assert_eq!(wind_h_at_start(100.5, 100.0), None);
    }

    #[test]
    fn the_wind_is_kept_keyed_and_said_to_the_same_tenth() {
        assert_eq!(tenths(24.24), 242.0);
        assert_eq!(to_tenth(24.26), 24.3);
        assert_eq!(to_tenth(0.04), 0.0);
    }

    #[test]
    fn a_takes_wind_is_set_and_cleared_in_every_entry() {
        let dir = std::env::temp_dir().join(format!("tg-full-wind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(take::MANIFEST);
        std::fs::write(
            &file,
            "# kept as typed\nwatch = \"Pepsi\"\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n\n\
             [[recording]]\nfile = \".\"\nposition = \"DD\"\nwind_h = 3.0\n",
        )
        .unwrap();
        set_take_wind_h(&dir, Some(24.24)).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("# kept as typed\n"), "{text}");
        let (m, _) = crate::take_path::take_of(&dir).unwrap().unwrap();
        assert!(m.recordings.iter().all(|r| r.wind_h == Some(24.2)));
        assert_eq!(take_wind_h(&m), Some(24.2));
        set_take_wind_h(&dir, None).unwrap();
        let (m, _) = crate::take_path::take_of(&dir).unwrap().unwrap();
        assert_eq!(take_wind_h(&m), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
