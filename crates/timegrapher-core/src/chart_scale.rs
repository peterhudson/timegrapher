//! The value scale a chart is drawn on, so the app, the MCP view and the
//! command line's reports scale the same readings the same way (#513).

/// From the lowest value to the highest, padded a little, so every point
/// drawn is inside the chart: the app's charts over time. `None` with no
/// finite value.
pub fn data_range(values: &[f64]) -> Option<(f64, f64)> {
    let (lo, hi) = values
        .iter()
        .copied()
        .filter(|y| y.is_finite())
        .fold(None, |r: Option<(f64, f64)>, y| {
            Some(r.map_or((y, y), |(lo, hi)| (lo.min(y), hi.max(y))))
        })?;
    let pad = ((hi - lo) * 0.08).max(0.05);
    Some((lo - pad, hi + pad))
}

/// The 1st to 99th percentile of the values, padded a little, leaving out
/// lone wild values (a knock, a dropped beat) so that two of them don't
/// flatten the rest: those beyond three interquartile ranges of the middle
/// half, unless there are more than two of them and more than 3% of the
/// values (then they are part of what the watch does, as in the steadiness
/// test). The steadiness views'
/// scale. `None` with fewer than two finite values.
pub fn spread(values: &[f64]) -> Option<(f64, f64)> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.len() < 2 {
        return None;
    }
    v.sort_by(f64::total_cmp);
    // Core's one quantile rule (#512), not a nearest rank of its own.
    let q = crate::dsp::quantile;
    let (q1, q3) = (q(&v, 0.25), q(&v, 0.75));
    let fence = 3.0 * (q3 - q1);
    let kept: Vec<f64> = v
        .iter()
        .copied()
        .filter(|&x| x >= q1 - fence && x <= q3 + fence)
        .collect();
    // One or two are always lone; more than that, 3% decides.
    let lone = (0.03 * v.len() as f64).max(2.0);
    if kept.len() >= 2 && (v.len() - kept.len()) as f64 <= lone {
        v = kept;
    }
    let (lo, hi) = (q(&v, 0.01), q(&v, 0.99));
    let pad = ((hi - lo) * 0.12).max(1e-3);
    Some((lo - pad, hi + pad))
}

/// The size a diverging colour map shows in full colour, either side of
/// zero: the 95th percentile of the values' sizes by core's one
/// percentile rule ([`crate::dsp::percentile`], #512), so a few wild cells
/// don't wash out the rest; larger ones show in full colour too. 1 with
/// no finite value, and never below 1e-9.
pub fn colour_limit(values: &[f64]) -> f64 {
    let sizes: Vec<f64> = values.iter().map(|x| x.abs()).collect();
    crate::dsp::percentile(&sizes, COLOUR_FULL_AT)
        .unwrap_or(1.0)
        .max(1e-9)
}

/// The share of a colour map's cells shown short of full colour.
const COLOUR_FULL_AT: f64 = 0.95;

/// Decimals for an axis label from its grid step: none from a step of
/// 1, one from 0.1, two from 0.01, three below that (#815). The app's
/// charts label their axes by it; the MCP view's `axisLabel`, which can't
/// call Rust, is held to the same fixture as [`axis_number`].
pub fn axis_decimals(step: f64) -> usize {
    if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else if step >= 0.01 {
        2
    } else {
        3
    }
}

/// An axis value as text, to [`axis_decimals`] of the step. A value that
/// rounds to zero is written `0`, never `-0` (#815).
pub fn axis_number(v: f64, step: f64) -> String {
    crate::library::fixed_text(v, axis_decimals(step))
}

/// 1, 2 or 5 times a power of ten, at least `x` (above 0): the step
/// every grid and axis rounds to, so there is one rule for round numbers.
/// The MCP view's `roundStep`, which can't call Rust, is held to the same
/// fixture.
pub fn round_step(x: f64) -> f64 {
    let p = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * p)
        .find(|&s| s >= x * (1.0 - 1e-9))
        .unwrap_or(10.0 * p)
}

/// How many steps a scale's span is cut into, about: the step is the
/// [`round_step`] at least a fifth of it.
const SCALE_STEPS: f64 = 5.0;

/// The grid step for a scale over `span`: the [`round_step`] at least a
/// fifth of it ([`SCALE_STEPS`]), so every chart's grid cuts its span the
/// same way.
pub fn step_for(span: f64) -> f64 {
    round_step((span / SCALE_STEPS).max(1e-12))
}

/// A scale with round ends: `lo` to `hi` snapped out to the nearest
/// [`round_step`] about a fifth of their span, and that step, as
/// `(lo, hi, step)`. −91.9 to −21.9 reads −100 to −20 in steps of 20.
pub fn round_ends(lo: f64, hi: f64) -> (f64, f64, f64) {
    round_ends_in(lo, hi, SCALE_STEPS)
}

/// [`round_ends`] with the step the [`round_step`] at least
/// `1 / steps` of the span, for a scale cut finer than a fifth.
pub fn round_ends_in(lo: f64, hi: f64, steps: f64) -> (f64, f64, f64) {
    let step = round_step(((hi - lo) / steps).max(1e-12));
    let lo = (lo / step + 1e-9).floor() * step;
    let hi = (hi / step - 1e-9).ceil() * step;
    // A single value still gets a step of room.
    let hi = if hi > lo { hi } else { lo + step };
    (lo + 0.0, hi + 0.0, step)
}

/// A scale that holds still: `shown` (from [`round_ends`]) while it holds
/// every value from `values.0` to `values.1` and the room `wanted` for
/// them fills at least half of it, so the scale doesn't jump as the
/// readings move within it; else new round ends around `wanted`. The ends
/// move out when a value leaves the scale, and in only when the room
/// wanted fills less than half of it. `wanted` holds the values with any
/// room around them (a least span, padding): only the values themselves
/// decide whether the scale still holds them, so room that slides with
/// them doesn't move the scale.
pub fn steady_ends(
    shown: Option<(f64, f64, f64)>,
    values: (f64, f64),
    wanted: (f64, f64),
) -> (f64, f64, f64) {
    let ((lo, hi), (w0, w1)) = (values, wanted);
    match shown {
        Some((a, b, step)) if lo >= a && hi <= b && (w1 - w0) * 2.0 >= b - a => (a, b, step),
        _ => round_ends(w0.min(lo), w1.max(hi)),
    }
}

/// One row of names in a lane above a plot, px.
pub const NAME_ROW_H: f64 = 15.0;
/// The short gap between a plot and its nearest row of names, so each
/// line or leader shows outside the plot before it reaches its name.
pub const NAME_LANE_GAP: f64 = 6.0;
/// The room kept between two names in any lane, side by side or one
/// above another: more than a line of a name (Peter on #1764), so two names
/// never read as one phrase.
pub const NAME_GAP: f64 = 12.0;
/// From one line of a name to the next, where a narrow lane breaks a name
/// a word to a line (#1764): the gap between two names ([`NAME_GAP`])
/// comes on top of it, so a name's own lines read as one.
pub const NAME_LINE_H: f64 = 13.0;
/// How far an overlap must start or end before a name changes row, so
/// names don't hop between frames.
pub const NAME_HYSTERESIS: f64 = 2.0;
/// The clear space kept between two names one above the other (a name a
/// row out over the one under it, or names stacked beside a map): a full
/// line, so two names never read as one (Peter on #1764). Within a name of
/// several lines, the lines sit at the normal line spacing.
pub const NAME_CLEAR: f64 = 11.0;
/// The pitch of a lane's second row: a row and the clear space, so a name
/// a row out never reads as part of the name under it (#1764).
pub const NAME_ROW_OUT: f64 = NAME_ROW_H + NAME_CLEAR;

/// The order names in a lane are placed in: strong first, then faint,
/// each left to right. `names` are each one's centre, width and whether
/// it is faint.
pub fn lane_order(names: &[(f64, f64, bool)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..names.len()).collect();
    order.sort_by(|&a, &b| {
        names[a]
            .2
            .cmp(&names[b].2)
            .then(names[a].0.total_cmp(&names[b].0))
    });
    order
}

/// Where each name in a lane of two rows above a plot goes, the named-lines
/// rule every surface draws by (the app's `marks::place`, the CLI's report,
/// held to the MCP view's copy by the same cases): `names` are each one's
/// centre (its line's x), width and whether it is faint; `was` is the row
/// each was in before. Strong names are placed first, then faint, each
/// left to right. Each is centred on its line within `left..right`, and
/// goes one row further out where it would come within [`NAME_GAP`] of a
/// name already in the first row; where the second row is taken there
/// too, it slides right of the name there (drawn with an angled leader).
/// Each name's left edge and row, in the order given.
pub fn lane_place(names: &[(f64, f64, bool)], left: f64, right: f64, was: &[u8]) -> Vec<(f64, u8)> {
    let order = lane_order(names);
    let mut rows: [Vec<(f64, f64)>; 2] = Default::default();
    let mut out = vec![(0.0, 0); names.len()];
    for i in order {
        let (x, w, _) = names[i];
        let last = (right - w).max(left);
        let at = (x - w / 2.0).clamp(left, last);
        // The overlap with a name in the first row, the gap between names
        // included: a name goes out once it overlaps by the hysteresis,
        // and comes back once it clears by as much.
        let overlap = rows[0]
            .iter()
            .map(|&(a, b)| (b + NAME_GAP - at).min(at + w + NAME_GAP - a))
            .fold(f64::NEG_INFINITY, f64::max);
        let out_row = if was.get(i).copied().unwrap_or(0) == 1 {
            overlap > -NAME_HYSTERESIS
        } else {
            overlap > NAME_HYSTERESIS
        };
        let (at, row) = if out_row {
            let mut a = at;
            while let Some(&(_, e)) = rows[1]
                .iter()
                .find(|&&(s, e)| a < e + NAME_GAP && a + w + NAME_GAP > s)
            {
                a = e + NAME_GAP;
            }
            (a.min(last), 1)
        } else {
            (at, 0)
        };
        rows[usize::from(row)].push((at, at + w));
        out[i] = (at, row);
    }
    out
}

/// Where each name in a lane of one row above a plot goes (the app's
/// `marks::place_row`), for a plot that names only a few lines: `names` as
/// [`lane_place`] takes them. Each is centred on its line within
/// `left..right`; one that would come within [`NAME_GAP`] of a name already
/// placed slides along past it (drawn with an angled leader), to the right
/// where there is room, else to the left. No leader then passes another
/// name. Each name's left edge, in the order given.
pub fn lane_place_row(names: &[(f64, f64, bool)], left: f64, right: f64) -> Vec<f64> {
    let order = lane_order(names);
    let mut taken: Vec<(f64, f64)> = Vec::new();
    let mut out = vec![0.0; names.len()];
    for i in order {
        let (x, w, _) = names[i];
        let last = (right - w).max(left);
        let centred = (x - w / 2.0).clamp(left, last);
        let hit = |a: f64, taken: &[(f64, f64)]| {
            taken
                .iter()
                .find(|&&(s, e)| a < e + NAME_GAP && a + w + NAME_GAP > s)
                .copied()
        };
        let mut at = centred;
        while let Some((_, e)) = hit(at, &taken) {
            at = e + NAME_GAP;
        }
        if at > last {
            at = centred;
            while let Some((s, _)) = hit(at, &taken) {
                at = s - NAME_GAP - w;
            }
            at = at.max(left);
        }
        taken.push((at, at + w));
        out[i] = at;
    }
    out
}

/// How much more a strong name holds to its line than a faint one, in a
/// column beside a plot: a wheel's name stays on its line and a mesh's or
/// ×2's makes way (Hugh on #1764).
pub const STRONG_HOLD: f64 = 100.0;

/// Where each name in a column beside a plot goes, along y (#1764):
/// `names` are each one's line's y, its height and whether it is faint.
/// The names keep their lines' order, so no two leaders cross, with
/// [`NAME_GAP`] clear between any two. Each run of names that would touch
/// sits where its names are nearest their lines, a strong one holding to
/// its line far more than a faint one, within `top..bottom`. Each name's
/// top, in the order given.
pub fn lane_stack(names: &[(f64, f64, bool)], top: f64, bottom: f64) -> Vec<f64> {
    let mut o: Vec<usize> = (0..names.len()).collect();
    o.sort_by(|&a, &b| names[a].0.total_cmp(&names[b].0));
    let hold = |i: usize| if names[i].2 { 1.0 } else { STRONG_HOLD };
    // A run, as its first and its count in `o`: its height, and its top
    // where its names sit nearest their lines, kept within the column.
    let height = |(s, n): (usize, usize)| {
        o[s..s + n].iter().map(|&i| names[i].1).sum::<f64>() + (n - 1) as f64 * NAME_GAP
    };
    let first = |(s, n): (usize, usize)| {
        let (mut off, mut sum, mut weight) = (0.0, 0.0, 0.0);
        for &i in &o[s..s + n] {
            let (y, h, _) = names[i];
            sum += hold(i) * (y - h / 2.0 - off);
            weight += hold(i);
            off += h + NAME_GAP;
        }
        (sum / weight).min(bottom - height((s, n))).max(top)
    };
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for k in 0..o.len() {
        let mut run = (k, 1);
        while let Some(&above) = runs.last() {
            if first(run) < first(above) + height(above) + NAME_GAP {
                runs.pop();
                run = (above.0, above.1 + run.1);
            } else {
                break;
            }
        }
        runs.push(run);
    }
    let mut out = vec![0.0; names.len()];
    for &run in &runs {
        let mut at = first(run);
        for &i in &o[run.0..run.0 + run.1] {
            out[i] = at;
            at += names[i].1 + NAME_GAP;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Names beside a plot keep their lines' order with a clear gap; a
    /// strong name holds its line and a faint one makes way (#1764).
    #[test]
    fn lane_stack_keeps_order_and_holds_strong_names() {
        // Apart: each centred on its line.
        let at = lane_stack(&[(100.0, 26.0, false), (200.0, 39.0, true)], 0.0, 400.0);
        assert_eq!(at, vec![87.0, 180.5]);
        // Touching: in order, the gap clear, the strong name all but on
        // its line.
        let at = lane_stack(&[(100.0, 26.0, false), (110.0, 39.0, true)], 0.0, 400.0);
        assert!((at[1] - (at[0] + 26.0 + NAME_GAP)).abs() < 1e-9, "{at:?}");
        assert!((at[0] - 87.0).abs() < 1.0, "{at:?}");
        // A faint name just above a strong one stays above it.
        let at = lane_stack(&[(100.0, 26.0, false), (95.0, 39.0, true)], 0.0, 400.0);
        assert!(at[1] + 39.0 + NAME_GAP <= at[0] + 1e-9, "{at:?}");
        // Kept within the column.
        let at = lane_stack(&[(5.0, 26.0, false), (8.0, 39.0, true)], 0.0, 400.0);
        assert_eq!(at[0], 0.0);
    }

    /// The lane rules place the cases the app's marks and the MCP view's
    /// copy are held to (timegrapher-app/src/marks_cases.json) as given.
    #[test]
    fn lane_place_places_the_shared_cases() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../timegrapher-app/src/marks_cases.json"
        );
        let cases: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("marks_cases.json"))
                .expect("its JSON");
        let n = |v: &serde_json::Value| v.as_f64().expect("a number");
        let list = cases["place"].as_array().expect("place");
        assert!(!list.is_empty());
        for c in list {
            let names: Vec<(f64, f64, bool)> = c["names"]
                .as_array()
                .expect("names")
                .iter()
                .map(|v| (n(&v[0]), n(&v[1]), v[2] == "faint"))
                .collect();
            let was: Vec<u8> = c["was"]
                .as_array()
                .expect("was")
                .iter()
                .map(|r| u8::try_from(r.as_u64().expect("a row")).expect("0 or 1"))
                .collect();
            let got = lane_place(&names, n(&c["left"]), n(&c["right"]), &was);
            let want = c["at"].as_array().expect("at");
            assert_eq!(got.len(), want.len(), "{}", c["case"]);
            for (g, w) in got.iter().zip(want) {
                assert!(
                    (g.0 - n(&w[0])).abs() < 1e-3 && u64::from(g.1) == w[1],
                    "{}: {got:?}",
                    c["case"]
                );
            }
        }
        let rows = cases["place_row"].as_array().expect("place_row");
        assert!(!rows.is_empty());
        for c in rows {
            let names: Vec<(f64, f64, bool)> = c["names"]
                .as_array()
                .expect("names")
                .iter()
                .map(|v| (n(&v[0]), n(&v[1]), v[2] == "faint"))
                .collect();
            let got = lane_place_row(&names, n(&c["left"]), n(&c["right"]));
            let want = c["at"].as_array().expect("at");
            assert_eq!(got.len(), want.len(), "{}", c["case"]);
            for (g, w) in got.iter().zip(want) {
                assert!((g - n(w)).abs() < 1e-3, "{}: {got:?}", c["case"]);
            }
        }
    }

    /// The axis rule, held to the fixture the MCP view's copy is tested
    /// against too.
    #[test]
    fn an_axis_label_takes_its_decimals_from_the_step_and_has_no_minus_zero() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../tests/fixtures/axis_numbers.json")).unwrap();
        assert!(!cases.is_empty());
        for c in cases {
            let (v, step) = (c["v"].as_f64().unwrap(), c["step"].as_f64().unwrap());
            assert_eq!(
                axis_number(v, step),
                c["text"].as_str().unwrap(),
                "{v} {step}"
            );
        }
    }

    #[test]
    fn a_round_step_is_one_two_or_five_times_a_power_of_ten() {
        // The MCP view's roundStep reads the same cases (round_steps.test.cjs).
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../tests/fixtures/round_steps.json")).unwrap();
        assert!(!cases.is_empty());
        for c in cases {
            let (x, step) = (c["x"].as_f64().unwrap(), c["step"].as_f64().unwrap());
            assert!((round_step(x) - step).abs() < step * 1e-9, "{x}");
        }
    }

    #[test]
    fn a_spread_reads_its_quartiles_and_ends_by_cores_one_quantile() {
        // Interpolated (type 7), as dsp::quantile reads them: the 99th
        // percentile of 0, 1, 2, 10 is 9.76, not the nearest rank's 10.
        let (lo, hi) = spread(&[10.0, 0.0, 2.0, 1.0]).unwrap();
        let pad = (9.76 - 0.03) * 0.12;
        assert!((lo - (0.03 - pad)).abs() < 1e-9, "{lo}");
        assert!((hi - (9.76 + pad)).abs() < 1e-9, "{hi}");
    }

    #[test]
    fn two_wild_values_are_lone_even_past_three_percent_of_a_short_series() {
        // 55 values, one wild at each end: 3.6%, yet still lone, so the
        // scale is the one without them.
        let calm: Vec<f64> = (0..53).map(|i| 0.4 + (i % 7) as f64 * 0.005).collect();
        let mut wild = calm.clone();
        wild.extend([-3.0, 1.4]);
        assert_eq!(spread(&wild), spread(&calm));
        // A third wild value past 3% is part of what the watch does.
        wild.push(-2.9);
        assert!(spread(&wild).unwrap().0 < spread(&calm).unwrap().0);
    }

    #[test]
    fn a_grid_step_cuts_its_span_in_about_five() {
        // At least a fifth of the span, rounded up to 1, 2 or 5.
        assert_eq!(step_for(10.0), 2.0);
        assert_eq!(step_for(12.0), 5.0);
        assert_eq!(step_for(0.7), 0.2);
        assert_eq!(step_for(500.0), 100.0);
        // A span of nothing still has a step.
        assert!(step_for(0.0) > 0.0);
    }

    #[test]
    fn a_scales_ends_snap_out_to_a_round_step() {
        // The spec's examples (Line Strength on Plots, the Paper Strip).
        assert_eq!(round_ends(-91.9, -21.9), (-100.0, -20.0, 20.0));
        assert_eq!(round_ends(222.0, 268.0), (220.0, 270.0, 10.0));
        let (lo, hi, step) = round_ends(-0.548, -0.302);
        assert!(
            (lo + 0.55).abs() < 1e-12 && (hi + 0.30).abs() < 1e-12 && (step - 0.05).abs() < 1e-12
        );
        // Ends already round stay.
        assert_eq!(round_ends(-100.0, -20.0), (-100.0, -20.0, 20.0));
        // One value: a step of room.
        assert!(round_ends(3.0, 3.0).1 > 3.0);
    }

    #[test]
    fn a_steady_scale_moves_only_when_it_must() {
        let shown = Some((-100.0, -20.0, 20.0));
        let at = |lo: f64, hi: f64| steady_ends(shown, (lo, hi), (lo, hi));
        // Readings move inside it: it holds.
        assert_eq!(at(-90.0, -30.0), (-100.0, -20.0, 20.0));
        // One leaves it: out.
        assert_eq!(at(-90.0, -15.0).1, 0.0);
        // They fill less than half of it: in.
        assert_eq!(at(-60.0, -40.0), round_ends(-60.0, -40.0));
        // Nothing shown yet: round ends.
        assert_eq!(
            steady_ends(None, (-91.9, -21.9), (-91.9, -21.9)),
            (-100.0, -20.0, 20.0)
        );
        // Room kept around steady readings slides with them past an end:
        // the readings are still inside, so the scale holds.
        let steady = Some((2.0, 8.0, 1.0));
        assert_eq!(
            steady_ends(steady, (4.9, 5.2), (2.05, 8.05)),
            (2.0, 8.0, 1.0)
        );
        assert_eq!(steady_ends(steady, (4.9, 5.2), (1.9, 7.9)), (2.0, 8.0, 1.0));
        // A reading leaves it: new ends around the room wanted.
        assert_eq!(
            steady_ends(steady, (4.9, 8.2), (1.9, 8.2)),
            round_ends(1.9, 8.2)
        );
    }

    #[test]
    fn a_charts_range_holds_every_point_padded() {
        let (lo, hi) = data_range(&[1.0, f64::NAN, 3.0, 2.0]).unwrap();
        assert!(
            (lo - 0.84).abs() < 1e-12 && (hi - 3.16).abs() < 1e-12,
            "{lo} {hi}"
        );
        assert_eq!(data_range(&[]), None);
        let (lo, hi) = data_range(&[3.0]).unwrap();
        assert!((lo - 2.95).abs() < 1e-12 && (hi - 3.05).abs() < 1e-12);
    }

    #[test]
    fn two_wild_values_dont_flatten_the_rest() {
        let mut v: Vec<f64> = (0..100).map(|i| (i % 10) as f64 * 0.1).collect();
        v[10] = 50.0;
        v[60] = -40.0;
        let (lo, hi) = spread(&v).unwrap();
        assert!(lo > -1.0 && hi < 2.0, "{lo} {hi}");
        // More than 3% of them is what the watch does: kept.
        for k in 0..5 {
            v[20 + k] = 50.0;
        }
        assert!(spread(&v).unwrap().1 > 40.0);
        assert_eq!(spread(&[1.0]), None);
    }

    #[test]
    fn a_colour_map_is_full_at_the_95th_percentile_of_sizes() {
        let v: Vec<f64> = (1..=100)
            .map(|i| if i % 2 == 0 { i as f64 } else { -(i as f64) })
            .collect();
        // Sizes 1 to 100: 95.05, between the 95th and 96th, as core's
        // percentile reads it (nearest rank read 96).
        assert!(
            (colour_limit(&v) - 95.05).abs() < 1e-9,
            "{}",
            colour_limit(&v)
        );
        assert_eq!(colour_limit(&[f64::NAN]), 1.0);
        assert_eq!(colour_limit(&[0.0, 0.0]), 1e-9);
    }
}
