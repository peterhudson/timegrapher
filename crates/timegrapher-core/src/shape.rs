//! The shape of one beat: its three sounds and what surrounds them.
//!
//! Each beat has three sounds, 1 unlock, 2 impulse and 3 drop, and beats
//! alternate between the two pallet stones. Many escapement faults change
//! the spacing, levels or separation of these sounds, or add sounds or
//! noise around them, on one stone or both (see `docs/fault-signatures.md`).
//!
//! As for amplitude, single beats are too noisy to read, so the shape is
//! measured on median envelope templates of each side, both over short
//! windows and over the whole recording. Only the 1-to-3 interval is also
//! read beat by beat, for its jitter.
//!
//! Sounds are found as rises rather than as peaks: a quiet unlock often
//! climbs to a shoulder and stays there until the impulse, so it has no
//! peak of its own. A rise counts when it climbs a set fraction of the
//! drop's height, and by a set ratio, within 0.6 ms; its time is where it
//! crosses halfway up. Sound 3 is the drop peak used by the amplitude
//! measurement, sound 1 the rise at the unlock edge, and sound 2 the biggest
//! rise between them. Each sound's level is the highest point in the 1 ms
//! after its rise. A sound that cannot be separated is `None`, which is
//! itself a finding ("1 and 2 not separable").

use crate::amplitude::edges;
use crate::beats::{median_window, Beat};
use crate::dsp::{median, median_f32, moving_average, parabolic, robust_sd};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ShapeConfig {
    /// Window for the windowed templates, seconds.
    pub window_s: f64,
    /// Template span before and after the beat's reference point, seconds.
    pub pre_s: f64,
    pub post_s: f64,
    /// Unlock threshold as a fraction of the drop's height (as for amplitude).
    pub onset_fraction: f32,
    /// Smallest rise that counts as a sound, as a fraction of the drop's
    /// height above the floor, and as a ratio to the level it rises from.
    pub min_prominence: f32,
    pub min_rise: f32,
    /// Stretch after the drop peak whose mean level is the tail, seconds.
    pub tail_s: (f64, f64),
    /// Extra events are searched for this far before the unlock and after
    /// the drop peak, leaving `extra_gap_s` clear of each. They must rise
    /// further than a sound within the beat, since the drop's decaying tail
    /// is lumpy.
    pub extra_span_s: f64,
    pub extra_gap_s: f64,
    pub extra_prominence: f32,
    /// Beats below this detection quality are left out.
    pub min_quality: f32,
}

impl Default for ShapeConfig {
    fn default() -> Self {
        ShapeConfig {
            window_s: 2.0,
            pre_s: 0.025,
            post_s: 0.025,
            onset_fraction: 0.02,
            min_prominence: 0.04,
            min_rise: 1.5,
            tail_s: (0.003, 0.015),
            extra_span_s: 0.010,
            extra_gap_s: 0.0015,
            extra_prominence: 0.08,
            min_quality: 0.4,
        }
    }
}

/// A sound outside the 1-to-3 span.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ExtraEvent {
    /// Time from the unlock edge, ms (negative before it).
    pub t_ms: f64,
    /// Peak level above the floor relative to the drop's.
    pub level: f64,
}

/// Shape measurements on one template. Times are from the unlock edge, ms;
/// levels are envelope heights above the template's floor (in power).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Shape {
    /// Unlock edge to drop edge, as the amplitude measurement reads it.
    pub unlock_to_drop_ms: f64,
    /// The unlock edge from the beat's reference point (template time).
    pub unlock_at_ms: f64,
    /// Where sounds 1, 2 and 3 cross halfway up their own rise.
    pub t1_ms: Option<f64>,
    pub t2_ms: Option<f64>,
    pub t3_ms: f64,
    /// Where each sound peaks.
    pub peak1_ms: Option<f64>,
    pub peak2_ms: Option<f64>,
    pub peak3_ms: f64,
    /// Intervals between the halfway crossings.
    pub i12_ms: Option<f64>,
    pub i23_ms: Option<f64>,
    pub i13_ms: Option<f64>,
    pub level1: Option<f64>,
    pub level2: Option<f64>,
    pub level3: f64,
    pub ratio13: Option<f64>,
    pub ratio23: Option<f64>,
    /// Lowest point between two sounds' peaks relative to the quieter
    /// peak: near 0 for well separated sounds, 1 when there is no dip.
    pub valley12: Option<f64>,
    pub valley23: Option<f64>,
    /// Mean level between two sounds' peaks relative to their mean.
    pub fill12: Option<f64>,
    pub fill23: Option<f64>,
    /// Mean level over `tail_s` after the drop peak relative to the drop.
    pub tail_ratio: f64,
    /// Noise between beats relative to the silence just before the unlock
    /// (needs the envelope, so `None` from [`measure`] alone).
    pub noise_ratio: Option<f64>,
    /// Rises from the unlock to the drop, counting the drop: 3 for a
    /// textbook beat, more when a sound is doubled.
    pub rises: usize,
    pub extra_pre: Vec<ExtraEvent>,
    pub extra_post: Vec<ExtraEvent>,
}

/// One rise of the envelope: where a sound starts.
#[derive(Debug, Clone, Copy)]
struct Rise {
    /// Halfway from `base` to `top`, fractional samples.
    at: f64,
    /// Where the rise starts from, and its level.
    from: usize,
    base: f32,
    top: f32,
}

/// Fractional position where `t` first exceeds `level` in `from..=to`.
fn crossing(t: &[f32], from: usize, to: usize, level: f32) -> Option<f64> {
    let i = (from.max(1)..=to.min(t.len() - 1)).find(|&i| t[i] > level)?;
    let (y0, y1) = (t[i - 1], t[i]);
    Some((i - 1) as f64 + ((level - y0) / (y1 - y0)) as f64)
}

/// Rises centred in `a..b` that climb at least `min_step` and at least
/// `min_ratio` times their base within 0.6 ms.
///
/// Candidates are the local maxima of the log envelope's slope, so a quiet
/// sound counts as much as a loud one; each is then measured from the
/// lowest point in the 0.6 ms before it to the highest in the 0.6 ms after.
/// Several candidates on one rise collapse to the biggest.
fn rises(t: &[f32], fs: f64, a: usize, b: usize, min_step: f32, min_ratio: f32) -> Vec<Rise> {
    let n = t.len();
    let h = ((0.0001 * fs) as usize).max(1);
    let w = ((0.0006 * fs) as usize).max(2);
    let l: Vec<f32> = t.iter().map(|&v| v.max(f32::MIN_POSITIVE).ln()).collect();
    let slope = |i: usize| l[(i + h).min(n - 1)] - l[i.saturating_sub(h)];
    let mut out: Vec<Rise> = Vec::new();
    for i in a.max(1)..b.min(n.saturating_sub(1)) {
        let s = slope(i);
        if !(s > 0.0 && s > slope(i - 1) && s >= slope(i + 1)) {
            continue;
        }
        let lo = i.saturating_sub(w);
        let (bi, base) = (lo..=i)
            .map(|k| (k, t[k]))
            .fold((i, t[i]), |m, p| if p.1 < m.1 { p } else { m });
        let Some((ti, top)) = crate::dsp::argmax(t, i, i + w + 1) else {
            continue;
        };
        if top - base < min_step || top < min_ratio * base {
            continue;
        }
        let Some(at) = crossing(t, bi + 1, ti, base + 0.5 * (top - base)) else {
            continue;
        };
        let r = Rise {
            at,
            from: bi,
            base,
            top,
        };
        match out.last_mut() {
            Some(last) if r.at - last.at < 0.5 * w as f64 => {
                if r.top - r.base > last.top - last.base {
                    *last = r;
                }
            }
            _ => out.push(r),
        }
    }
    out
}

/// Landmarks of a measured template, in template samples.
#[derive(Debug, Clone, Copy)]
struct Marks {
    /// Unlock edge, and the halfway crossings of sounds 1 and 3.
    unlock: f64,
    at1: Option<f64>,
    at3: f64,
}

/// Measure a template whose beat reference point sits `origin` samples in.
pub fn measure(template: &[f32], fs: f64, origin: usize, cfg: &ShapeConfig) -> Option<Shape> {
    measure_at(template, fs, origin, cfg).map(|m| m.0)
}

fn measure_at(
    template: &[f32],
    fs: f64,
    origin: usize,
    cfg: &ShapeConfig,
) -> Option<(Shape, Marks)> {
    if template.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let e = edges(template, fs, origin, cfg.onset_fraction)?;
    // The same light smoothing the edges are read on.
    let t = moving_average(template, ((0.0002 * fs) as usize).max(1));
    let (floor, height) = (e.floor, e.peak_level - e.floor);
    let samples = |s: f64| (s * fs) as usize;
    let ms = |i: f64| (i - e.unlock) / fs * 1000.0;
    // Levels above the floor. Noise adds to a sound in power rather than in
    // level, so the floor is taken off in power; otherwise quiet sounds
    // would read low against the drop.
    let above = |v: f32| ((v * v - floor * floor).max(0.0) as f64).sqrt();
    let a: Vec<f64> = t.iter().map(|&v| above(v)).collect();
    let lvl = |v: f32| above(v) / above(e.peak_level);
    let min_step = cfg.min_prominence * height;

    // Sound 3: the drop, from the lowest point in the 2 ms before its peak.
    let p3 = e.peak;
    let lo = p3.saturating_sub(samples(0.002));
    let (b3, base3) = (lo..=p3)
        .map(|k| (k, t[k]))
        .fold((p3, t[p3]), |m, p| if p.1 < m.1 { p } else { m });
    let at3 = crossing(&t, b3 + 1, p3, base3 + 0.5 * (e.peak_level - base3))?;

    // Every rise from just before the unlock edge to just before the drop.
    let gap = samples(0.0003) as f64;
    let first = (e.unlock - samples(0.0005) as f64).max(0.0) as usize;
    let inner: Vec<Rise> = rises(&t, fs, first, at3 as usize, min_step, cfg.min_rise)
        .into_iter()
        .filter(|r| r.at < at3 - gap)
        .collect();
    // Sound 1 must start at the unlock edge; sound 2 is the biggest rise after it.
    let s1 = inner
        .first()
        .copied()
        .filter(|r| r.at <= e.unlock + samples(0.001) as f64);
    let after1 = s1.map_or(e.unlock, |r| r.at + gap);
    let s2 = inner
        .iter()
        .copied()
        .filter(|r| r.at > after1)
        .max_by(|a, b| (a.top - a.base).total_cmp(&(b.top - b.base)));

    // Each sound's peak lies between its own rise and the start of the next.
    let reach = samples(0.001);
    let peak_in = |from: f64, to: usize| {
        crate::dsp::argmax(&t, from as usize, to.min(from as usize + reach) + 1)
    };
    let pk1 = s1.and_then(|r| peak_in(r.at, s2.map_or(b3, |r| r.from)));
    let pk2 = s2.and_then(|r| peak_in(r.at, b3));
    let pk3 = (p3, e.peak_level);
    let at = |i: usize| parabolic(&t, i);
    let between = |p: (usize, f32), q: (usize, f32)| -> (f64, f64) {
        let seg = &a[p.0..=q.0];
        let low = seg.iter().copied().fold(f64::INFINITY, f64::min);
        let mean = seg.iter().sum::<f64>() / seg.len() as f64;
        let (lp, lq) = (a[p.0], a[q.0]);
        (low / lp.min(lq), mean / ((lp + lq) / 2.0))
    };
    let v12 = pk1.zip(pk2).map(|(a, b)| between(a, b));
    let v23 = pk2.map(|a| between(a, pk3));

    // Tail: mean level over a stretch after the drop peak.
    let ta = p3 + samples(cfg.tail_s.0);
    let tb = (p3 + samples(cfg.tail_s.1)).min(t.len());
    let tail_ratio = if ta < tb {
        a[ta..tb].iter().sum::<f64>() / (tb - ta) as f64 / a[p3]
    } else {
        f64::NAN
    };

    // Extra events: rises a little way outside the 1-to-3 span.
    let extra = |a: f64, b: f64| -> Vec<ExtraEvent> {
        if b <= a {
            return Vec::new();
        }
        let step = cfg.extra_prominence * height;
        rises(&t, fs, a as usize, b as usize, step, cfg.min_rise)
            .into_iter()
            .map(|r| ExtraEvent {
                t_ms: ms(r.at),
                level: lvl(r.top),
            })
            .collect()
    };
    let quiet = samples(0.003) as f64;
    let span = samples(cfg.extra_span_s) as f64;
    let egap = samples(cfg.extra_gap_s) as f64;
    let extra_pre = extra((e.unlock - span).max(quiet), e.unlock - egap);
    let extra_post = extra(p3 as f64 + egap, (p3 as f64 + span).min(t.len() as f64));

    let t1 = s1.map(|r| r.at);
    let t2 = s2.map(|r| r.at);
    let d = |a: f64, b: f64| (b - a) / fs * 1000.0;
    let shape = Shape {
        unlock_to_drop_ms: d(e.unlock, e.drop),
        unlock_at_ms: d(origin as f64, e.unlock),
        t1_ms: t1.map(ms),
        t2_ms: t2.map(ms),
        t3_ms: ms(at3),
        peak1_ms: pk1.map(|p| ms(at(p.0))),
        peak2_ms: pk2.map(|p| ms(at(p.0))),
        peak3_ms: ms(at(p3)),
        i12_ms: t1.zip(t2).map(|(a, b)| d(a, b)),
        i23_ms: t2.map(|b| d(b, at3)),
        i13_ms: t1.map(|a| d(a, at3)),
        level1: pk1.map(|p| a[p.0]),
        level2: pk2.map(|p| a[p.0]),
        level3: a[p3],
        ratio13: pk1.map(|p| lvl(p.1)),
        ratio23: pk2.map(|p| lvl(p.1)),
        valley12: v12.map(|v| v.0),
        valley23: v23.map(|v| v.0),
        fill12: v12.map(|v| v.1),
        fill23: v23.map(|v| v.1),
        tail_ratio,
        noise_ratio: None,
        rises: inner.iter().filter(|r| r.at >= e.unlock - gap).count() + 1,
        extra_pre,
        extra_post,
    };
    let marks = Marks {
        unlock: e.unlock,
        at1: t1,
        at3,
    };
    Some((shape, marks))
}

/// Median over beats of each beat's median envelope level from `a` to `b`
/// seconds after its reference point.
fn stretch_level(env: &[f32], fs: f64, times: &[f64], a: f64, b: f64) -> Option<f64> {
    let mut levels: Vec<f64> = times
        .iter()
        .filter_map(|&t| {
            let i = ((t + a) * fs).round();
            let j = ((t + b) * fs).round();
            if i < 0.0 || j as usize > env.len() || j <= i {
                return None;
            }
            let mut seg = env[i as usize..j as usize].to_vec();
            Some(median_f32(&mut seg) as f64)
        })
        .collect();
    (!levels.is_empty()).then(|| median(&mut levels))
}

/// Shape of one side's beats, with the noise between beats, plus the
/// template and its landmarks.
fn side_shape(
    env: &[f32],
    fs: f64,
    times: &[f64],
    beat_s: f64,
    cfg: &ShapeConfig,
) -> Option<(Shape, Marks, Vec<f32>)> {
    if times.len() < 3 {
        return None;
    }
    let tmpl = median_window(env, fs, times, cfg.pre_s, cfg.post_s);
    let origin = (cfg.pre_s * fs).round() as usize;
    let (mut s, marks) = measure_at(&tmpl, fs, origin, cfg)?;
    // Silence just before the unlock against the middle of the gap after the drop.
    let u = marks.unlock / fs - cfg.pre_s;
    let pre = stretch_level(env, fs, times, u - 0.004, u - 0.001)?;
    let mid = stretch_level(env, fs, times, 0.35 * beat_s, 0.65 * beat_s)?;
    s.noise_ratio = (pre > 0.0).then_some(mid / pre);
    Some((s, marks, tmpl))
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeWindow {
    pub start_s: f64,
    pub end_s: f64,
    pub even: Option<Shape>,
    pub odd: Option<Shape>,
}

/// Beat-by-beat spread of the 1-to-3 interval on one side.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Spread {
    pub beats: usize,
    /// Median 1-to-3 interval over single beats, ms.
    pub i13_ms: f64,
    /// Robust standard deviation of single beats about their window's
    /// median, microseconds.
    pub i13_sd_us: f64,
}

/// One side over the whole recording.
#[derive(Debug, Clone, Serialize)]
pub struct SideReport {
    /// Median of each measure over the windows where it was found.
    pub windows: Shape,
    /// Windows measured, and those in which sound 1, sound 2 and an extra
    /// event before or after the beat were found.
    pub windows_measured: usize,
    pub windows_with_1: usize,
    pub windows_with_2: usize,
    pub windows_with_extra_pre: usize,
    pub windows_with_extra_post: usize,
    /// The whole-recording template. Amplitude changes during the
    /// recording smear the sounds before the drop, so the windowed
    /// medians are the better reading of timing.
    pub whole: Option<Shape>,
    pub spread: Option<Spread>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeReport {
    pub even: SideReport,
    pub odd: SideReport,
    pub windows: Vec<ShapeWindow>,
    /// Whole-recording templates, sample by sample from `-pre_s`.
    #[serde(skip)]
    pub template_even: Vec<f32>,
    #[serde(skip)]
    pub template_odd: Vec<f32>,
    pub template_start_ms: f64,
}

fn is_even(b: &Beat) -> bool {
    b.index.rem_euclid(2) == 0
}

/// How far a single beat's rise near template sample `at` sits from the
/// template's, in samples. The template's rise (0.4 ms either side of
/// `at`, mean removed) is slid over the beat up to `lag` samples each way;
/// `start` is the envelope sample where this beat's template would begin.
fn beat_offset(
    env: &[f32],
    fs: f64,
    start: isize,
    tmpl: &[f32],
    at: f64,
    lag: usize,
) -> Option<f64> {
    let w = (0.0004 * fs) as usize;
    let c = at.round() as usize;
    let seg = tmpl.get(c.checked_sub(w)?..c + w + 1)?;
    let mean = seg.iter().sum::<f32>() / seg.len() as f32;
    let first = start + (c - w) as isize - lag as isize;
    let last = start + (c + w) as isize + lag as isize;
    if first < 0 || last as usize >= env.len() {
        return None;
    }
    let score: Vec<f32> = (0..=2 * lag)
        .map(|k| {
            let o = (first + k as isize) as usize;
            seg.iter().zip(&env[o..]).map(|(a, b)| (a - mean) * b).sum()
        })
        .collect();
    let (i, _) = crate::dsp::argmax(&score, 0, score.len())?;
    Some(parabolic(&score, i) - lag as f64)
}

/// Shape of both sides in windows and over the whole recording.
pub fn analyze(
    env: &[f32],
    fs: f64,
    beats: &[Beat],
    beat_s: f64,
    cfg: &ShapeConfig,
) -> ShapeReport {
    let good: Vec<&Beat> = beats
        .iter()
        .filter(|b| b.quality > cfg.min_quality)
        .collect();
    let origin = (cfg.pre_s * fs).round() as usize;
    let side_times = |win: &[&Beat], even: bool| -> Vec<f64> {
        win.iter()
            .filter(|b| is_even(b) == even)
            .map(|b| b.time)
            .collect()
    };

    let mut windows = Vec::new();
    // Single-beat 1-to-3 deviations from each window's template, per side.
    let mut beat_i13: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    let mut beat_dev: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    if let (Some(first), Some(last)) = (good.first(), good.last()) {
        let mut start = first.time;
        let mut lo = 0usize;
        while start + cfg.window_s <= last.time + 1e-9 {
            while lo < good.len() && good[lo].time < start {
                lo += 1;
            }
            let mut hi = lo;
            while hi < good.len() && good[hi].time < start + cfg.window_s {
                hi += 1;
            }
            let win = &good[lo..hi];
            let mut pair = [None, None];
            for (k, even) in [true, false].into_iter().enumerate() {
                let times = side_times(win, even);
                let s = side_shape(env, fs, &times, beat_s, cfg);
                if let Some((_, m, tmpl)) = &s {
                    if let Some(a1) = m.at1 {
                        // Single beats: each one's rises 1 and 3 against the template's.
                        let lag = (0.0004 * fs) as usize;
                        let i13: Vec<f64> = times
                            .iter()
                            .filter_map(|&t| {
                                let start = (t * fs).round() as isize - origin as isize;
                                let o1 = beat_offset(env, fs, start, tmpl, a1, lag)?;
                                let o3 = beat_offset(env, fs, start, tmpl, m.at3, lag)?;
                                Some((m.at3 + o3 - a1 - o1) / fs * 1000.0)
                            })
                            .collect();
                        if i13.len() >= 3 {
                            let m = median(&mut i13.clone());
                            beat_dev[k].extend(i13.iter().map(|v| (v - m) * 1000.0));
                            beat_i13[k].extend(i13);
                        }
                    }
                }
                pair[k] = s.map(|s| s.0);
            }
            let [even, odd] = pair;
            windows.push(ShapeWindow {
                start_s: start,
                end_s: start + cfg.window_s,
                even,
                odd,
            });
            start += cfg.window_s;
        }
    }

    let whole = |even: bool| -> (Vec<f32>, Option<Shape>) {
        let times = side_times(&good, even);
        let tmpl = median_window(env, fs, &times, cfg.pre_s, cfg.post_s);
        (tmpl, side_shape(env, fs, &times, beat_s, cfg).map(|s| s.0))
    };
    let (template_even, whole_even) = whole(true);
    let (template_odd, whole_odd) = whole(false);

    let report = |k: usize, whole: Option<Shape>| -> SideReport {
        let shapes: Vec<&Shape> = windows
            .iter()
            .filter_map(|w| {
                if k == 0 {
                    w.even.as_ref()
                } else {
                    w.odd.as_ref()
                }
            })
            .collect();
        let spread = (beat_i13[k].len() >= 10).then(|| Spread {
            beats: beat_i13[k].len(),
            i13_ms: median(&mut beat_i13[k].clone()),
            i13_sd_us: robust_sd(&beat_dev[k]),
        });
        SideReport {
            windows: median_shape(&shapes),
            windows_measured: shapes.len(),
            windows_with_1: shapes.iter().filter(|s| s.t1_ms.is_some()).count(),
            windows_with_2: shapes.iter().filter(|s| s.t2_ms.is_some()).count(),
            windows_with_extra_pre: shapes.iter().filter(|s| !s.extra_pre.is_empty()).count(),
            windows_with_extra_post: shapes.iter().filter(|s| !s.extra_post.is_empty()).count(),
            whole,
            spread,
        }
    };
    ShapeReport {
        even: report(0, whole_even),
        odd: report(1, whole_odd),
        windows,
        template_even,
        template_odd,
        template_start_ms: -cfg.pre_s * 1000.0,
    }
}

/// Median of each measure over the shapes where it was found. Extra events
/// are not summarised here (see the window counts).
fn median_shape(shapes: &[&Shape]) -> Shape {
    let opt = |f: &dyn Fn(&Shape) -> Option<f64>| -> Option<f64> {
        let mut v: Vec<f64> = shapes
            .iter()
            .filter_map(|s| f(s))
            .filter(|x| x.is_finite())
            .collect();
        (!v.is_empty()).then(|| median(&mut v))
    };
    let all = |f: &dyn Fn(&Shape) -> f64| opt(&|s| Some(f(s))).unwrap_or(f64::NAN);
    Shape {
        unlock_to_drop_ms: all(&|s| s.unlock_to_drop_ms),
        unlock_at_ms: all(&|s| s.unlock_at_ms),
        t1_ms: opt(&|s| s.t1_ms),
        t2_ms: opt(&|s| s.t2_ms),
        t3_ms: all(&|s| s.t3_ms),
        peak1_ms: opt(&|s| s.peak1_ms),
        peak2_ms: opt(&|s| s.peak2_ms),
        peak3_ms: all(&|s| s.peak3_ms),
        i12_ms: opt(&|s| s.i12_ms),
        i23_ms: opt(&|s| s.i23_ms),
        i13_ms: opt(&|s| s.i13_ms),
        level1: opt(&|s| s.level1),
        level2: opt(&|s| s.level2),
        level3: all(&|s| s.level3),
        ratio13: opt(&|s| s.ratio13),
        ratio23: opt(&|s| s.ratio23),
        valley12: opt(&|s| s.valley12),
        valley23: opt(&|s| s.valley23),
        fill12: opt(&|s| s.fill12),
        fill23: opt(&|s| s.fill23),
        tail_ratio: all(&|s| s.tail_ratio),
        noise_ratio: opt(&|s| s.noise_ratio),
        rises: all(&|s| s.rises as f64).round() as usize,
        extra_pre: Vec::new(),
        extra_post: Vec::new(),
    }
}
