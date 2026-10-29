//! Each synthetic edge take, read as every surface reads a take
//! ([`take::measure_entry`]): it opens, carries the caveats it must, and
//! where it holds enough of the watch, reads the watch it was made from.

use timegrapher_core::edge_takes;
use timegrapher_core::session::Manifest;
use timegrapher_core::stream::StreamConfig;
use timegrapher_core::take;

#[test]
fn every_edge_take_reads_as_it_was_made() {
    let root = std::env::temp_dir().join(format!("tg-edge-takes-{}", std::process::id()));
    let takes = edge_takes::write_all(&root).unwrap();
    let mut wrong = Vec::new();
    for t in &takes {
        let dir = root.join("takes").join(t.id);
        let m: Manifest =
            toml::from_str(&std::fs::read_to_string(dir.join("session.toml")).unwrap()).unwrap();
        let mut stream = StreamConfig::default();
        stream.analysis.bph = m.bph;
        stream.analysis.amplitude.lift_deg = m.lift.unwrap();
        stream.analysis.lift_known = true;
        let s = take::EntrySettings {
            stream,
            card_clock: false,
            settle_s: None,
        };
        let read = take::measure_entry(&dir, &m, &m.recordings[0], &s, |_| Ok(None), |_| Ok(None))
            .unwrap_or_else(|e| panic!("{}: {e}", t.id));
        let m = &read.measurement;
        let mut here = Vec::new();
        let codes: Vec<String> = m
            .caveats
            .iter()
            .map(|c| {
                serde_json::to_value(c).unwrap()["code"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        for c in t.caveats {
            if !codes.iter().any(|k| k == c) {
                here.push(format!("{}: no {c} among {codes:?}", t.id));
            }
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
