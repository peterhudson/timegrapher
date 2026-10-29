//! Each synthetic edge take, read as every surface reads a take
//! ([`edge_takes::read`]): each entry opens, carries the caveats it must,
//! and where it holds enough of the watch, reads the watch it was made
//! from.

use timegrapher_core::edge_takes::{self, Truth};
use timegrapher_core::session::Measurement;

/// Where `m` differs from what the take `t` was made to read, in words.
fn differences(t: &edge_takes::EdgeTake, n: usize, m: &Measurement) -> Vec<String> {
    let mut out = Vec::new();
    let codes = edge_takes::caveat_codes(m);
    let made: &[&str] = t.caveats.get(n).copied().unwrap_or_default();
    let mut want: Vec<&str> = made.to_vec();
    let mut got: Vec<&str> = codes.iter().map(String::as_str).collect();
    want.sort_unstable();
    got.sort_unstable();
    if got != want {
        out.push(format!("caveats {codes:?}, made to carry {made:?}"));
    }
    if let Some(Truth {
        rate,
        amplitude,
        beat_error,
    }) = t.truth
    {
        // Near what was made, or none where none was.
        let near = |got: Option<f64>, want: Option<f64>, by: f64| match (got, want) {
            (Some(g), Some(w)) => (g - w).abs() <= by,
            (got, want) => got.is_none() && want.is_none(),
        };
        if !near(m.rate_s_per_day, Some(rate), 0.5)
            || !near(m.amplitude_deg, amplitude, 5.0)
            || !near(m.beat_error_unlock_ms.map(f64::abs), beat_error, 0.1)
        {
            out.push(format!(
                "read {:?} s/d, {:?}°, {:?} ms; made {rate} s/d, {amplitude:?}°, {beat_error:?} ms",
                m.rate_s_per_day, m.amplitude_deg, m.beat_error_unlock_ms
            ));
        }
    }
    out
}

#[test]
fn every_edge_take_reads_as_it_was_made() {
    let root = std::env::temp_dir().join(format!("tg-edge-takes-{}", std::process::id()));
    let takes = edge_takes::write_all(&root).unwrap();
    let mut wrong = Vec::new();
    for t in &takes {
        let reads = edge_takes::read(&root, t).unwrap_or_else(|e| panic!("{e}"));
        let mut here = Vec::new();
        if reads.len() != t.caveats.len() {
            here.push(format!(
                "{}: {} readings, made with {}",
                t.id,
                reads.len(),
                t.caveats.len()
            ));
        }
        for (n, read) in reads.iter().enumerate() {
            let m = &read.measurement;
            eprintln!(
                "{} #{}: rate {:?} amplitude {:?} beat error {:?} {:?} breaks {:?}",
                t.id,
                n + 1,
                m.rate_s_per_day,
                m.amplitude_deg,
                m.beat_error_unlock_ms,
                edge_takes::caveat_codes(m),
                read.breaks
            );
            here.extend(
                differences(t, n, m)
                    .into_iter()
                    .map(|d| format!("{} #{}: {d}", t.id, n + 1)),
            );
        }
        match t.known {
            None => wrong.extend(here),
            Some(why) if here.is_empty() => wrong.push(format!(
                "{}: now reads as it was made; take it off the known list ({why})",
                t.id
            )),
            Some(_) => {}
        }
    }
    std::fs::remove_dir_all(&root).ok();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
