//! The marks on a period axis drawn on a log scale, so the app and the
//! MCP server's views label it alike: times a watchmaker reads, each in
//! its own unit (5 s, 1 min, 1 h, 24 h), not the powers of ten in seconds.

/// One mark: its period, its label, and its level, 0 for the marks always
/// shown and 1 and 2 for those added as the axis has room.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PeriodTick {
    pub period_s: f64,
    pub label: &'static str,
    pub level: u8,
}

const fn tick(period_s: f64, label: &'static str, level: u8) -> PeriodTick {
    PeriodTick {
        period_s,
        label,
        level,
    }
}

/// Every mark, shortest first.
pub const TICKS: [PeriodTick; 19] = [
    tick(0.1, "0.1 s", 0),
    tick(0.2, "0.2 s", 2),
    tick(0.5, "0.5 s", 1),
    tick(1.0, "1 s", 0),
    tick(2.0, "2 s", 2),
    tick(5.0, "5 s", 1),
    tick(10.0, "10 s", 0),
    tick(30.0, "30 s", 1),
    tick(60.0, "1 min", 0),
    tick(120.0, "2 min", 2),
    tick(300.0, "5 min", 1),
    tick(600.0, "10 min", 0),
    tick(1800.0, "30 min", 1),
    tick(3600.0, "1 h", 0),
    tick(7200.0, "2 h", 2),
    tick(21600.0, "6 h", 0),
    tick(43200.0, "12 h", 1),
    tick(86400.0, "24 h", 0),
    tick(172800.0, "48 h", 2),
];

/// The marks between `lo_s` and `hi_s`: the finest level whose marks
/// number no more than `room`, and at least the level 0 marks.
pub fn period_ticks(lo_s: f64, hi_s: f64, room: usize) -> Vec<PeriodTick> {
    let inside = |t: &&PeriodTick| t.period_s >= lo_s && t.period_s <= hi_s;
    let at = |level: u8| -> Vec<PeriodTick> {
        TICKS
            .iter()
            .filter(inside)
            .filter(|t| t.level <= level)
            .copied()
            .collect()
    };
    (0..=2u8)
        .rev()
        .map(at)
        .find(|v| v.len() <= room)
        .unwrap_or_else(|| at(0))
}

/// The label of the mark at `period_s`, if one is there.
pub fn period_tick_label(period_s: f64) -> Option<&'static str> {
    TICKS
        .iter()
        .find(|t| (t.period_s / period_s - 1.0).abs() < 1e-6)
        .map(|t| t.label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_in_watchmakers_units_and_thin_out_to_fit() {
        let labels = |v: Vec<PeriodTick>| v.iter().map(|t| t.label).collect::<Vec<_>>().join(", ");
        // A whole night: wide room shows every mark, little room the main ones.
        assert_eq!(
            labels(period_ticks(4.0, 60_000.0, 20)),
            "5 s, 10 s, 30 s, 1 min, 2 min, 5 min, 10 min, 30 min, 1 h, 2 h, 6 h, 12 h"
        );
        assert_eq!(
            labels(period_ticks(4.0, 60_000.0, 10)),
            "5 s, 10 s, 30 s, 1 min, 5 min, 10 min, 30 min, 1 h, 6 h, 12 h"
        );
        assert_eq!(
            labels(period_ticks(4.0, 60_000.0, 6)),
            "10 s, 1 min, 10 min, 1 h, 6 h"
        );
        assert_eq!(period_tick_label(3600.0), Some("1 h"));
        assert_eq!(period_tick_label(1000.0), None);
        // Shortest first, no two alike.
        assert!(TICKS.windows(2).all(|w| w[0].period_s < w[1].period_s));
    }
}
