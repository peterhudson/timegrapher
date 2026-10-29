//! Each synthetic edge take, read as every surface reads a take
//! ([`edge_takes::read`]): it opens, carries the caveats it must, and
//! where it holds enough of the watch, reads the watch it was made from.

use timegrapher_core::edge_takes;

#[test]
fn every_edge_take_reads_as_it_was_made() {
    let root = std::env::temp_dir().join(format!("tg-edge-takes-{}", std::process::id()));
    let takes = edge_takes::write_all(&root).unwrap();
    let mut wrong = Vec::new();
    for t in &takes {
        let read = edge_takes::read(&root, t).unwrap_or_else(|e| panic!("{e}"));
        let m = &read.measurement;
        let mut here = Vec::new();
        let codes = edge_takes::caveat_codes(m);
        let mut want: Vec<&str> = t.caveats.to_vec();
        let mut got: Vec<&str> = codes.iter().map(String::as_str).collect();
        want.sort_unstable();
        got.sort_unstable();
        if got != want {
            here.push(format!(
                "{}: caveats {codes:?}, made to carry {:?}",
                t.id, t.caveats
            ));
        }
        eprintln!(
            "{}: rate {:?} amplitude {:?} beat error {:?} {codes:?} breaks {:?}",
            t.id, m.rate_s_per_day, m.amplitude_deg, m.beat_error_unlock_ms, read.breaks
        );
        if let Some((rate, amplitude, beat_error)) = t.truth {
            let near =
                |got: Option<f64>, want: f64, by: f64| got.is_some_and(|g| (g - want).abs() <= by);
            if !near(m.rate_s_per_day, rate, 0.5)
                || !near(m.amplitude_deg, amplitude, 5.0)
                || !near(m.beat_error_unlock_ms.map(f64::abs), beat_error, 0.1)
            {
                here.push(format!(
                    "{}: read {:?} s/d, {:?}°, {:?} ms; made {rate} s/d, {amplitude}°, {beat_error} ms",
                    t.id, m.rate_s_per_day, m.amplitude_deg, m.beat_error_unlock_ms
                ));
            }
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
