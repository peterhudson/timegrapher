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
    let mut d = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.splitn(3, ':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: f64 = t.next().unwrap_or("0").parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since 1970-01-01 from a civil date (Howard Hinnant's
    // algorithm), the inverse of the one `utc` uses.
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + hour * 3600 + minute * 60) as f64 + second)
}

/// Hours from full wind at `wound_unix` to `at_unix`, to a tenth: never
/// less than full wind.
pub fn hours_since(wound_unix: f64, at_unix: f64) -> f64 {
    (((at_unix - wound_unix) / 3600.0).max(0.0) * 10.0).round() / 10.0
}

/// A take's hours since full wind, as its session file gives them: its
/// first entry's that has them, or none where the wind wasn't given.
pub fn take_wind_h(m: &Manifest) -> Option<f64> {
    m.recordings.iter().find_map(|r| r.wind_h)
}

/// Give the take `take` its hours since full wind, `wind_h` (none where
/// it isn't given), in every entry of its session file, its own wording
/// kept: Set Wind… on a take in Watches.
pub fn set_take_wind_h(take: &Path, wind_h: Option<f64>) -> Result<(), String> {
    let file = take.join(take::MANIFEST);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
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
                e.insert("wind_h", toml_edit::value((h * 10.0).round() / 10.0));
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
    fn hours_since_full_wind_are_to_a_tenth_and_never_before_it() {
        assert_eq!(hours_since(0.0, 2.0 * 3600.0 + 170.0), 2.0);
        assert_eq!(hours_since(0.0, 24.24 * 3600.0), 24.2);
        assert_eq!(hours_since(100.0, 50.0), 0.0);
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
