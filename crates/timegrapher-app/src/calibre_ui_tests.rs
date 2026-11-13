//! The calibre picker, manager and store, headless.

use super::*;

/// A watch's own stored lift outside the one range is set aside, as a
/// calibre's is: the take starts on the default.
#[test]
fn a_watchs_lift_outside_the_range_is_set_aside() {
    let cal = Calibres::in_memory();
    for (lift, kept) in [(120.0, None), (5.0, None), (48.0, Some(48.0))] {
        let mut w = timegrapher_core::library::Watch::new("Test");
        w.lift = Some(lift);
        assert_eq!(cal.watch_lift(&w), kept, "{lift}");
    }
}

/// A source reads as its name, its link and what it says, without
/// the calibre's own name the panel is headed by.
#[test]
fn a_source_is_its_name_and_what_it_says() {
    assert_eq!(
        source_parts("https://watchguy.co.uk/cgi-bin/lift_angles (A. Schild [AS] 1017, 51°)"),
        (
            "watchguy.co.uk".to_string(),
            Some("https://watchguy.co.uk/cgi-bin/lift_angles".to_string()),
            "51°".to_string()
        )
    );
    let (name, _, says) = source_parts(
        "https://docs.google.com/spreadsheets/d/x (lift angle sheet linked from \
         watchrepairtalk.com/topic/27749-lift-angle) (IWC (International Watch Company) \
         45, 51°, 21600 vph)",
    );
    assert_eq!(name, "Lift angle sheet (watchrepairtalk.com)");
    assert_eq!(says, "51°, 21600 vph");
    let (name, url, says) = source_parts("https://www.sellita.ch/en/sw200");
    assert_eq!((name.as_str(), says.as_str()), ("sellita.ch", ""));
    assert!(url.is_some());
    let (name, url, _) = source_parts("project's bench recordings: a 5.00 s rate");
    assert_eq!(name, "project's bench recordings: a 5.00 s rate");
    assert!(url.is_none());
}

/// The search over the table finds a calibre by maker and name
/// together, the count says how many it shows, and a missing figure
/// is left to the cell's dash, not written into the filter's values as
/// a degree.
#[test]
fn the_table_search_finds_maker_and_calibre_together() {
    let mut cal = Calibres::in_memory();
    let all = cal.row_names().len();
    cal.search = "schild 1017".into();
    let found = cal.row_names();
    assert!(
        found.iter().any(|(_, n)| n == "A. Schild 1017"),
        "{found:?}"
    );
    assert!(found.len() < 5, "{found:?}");
    cal.search.clear();
    assert_eq!(cal.row_names().len(), all);
    assert_eq!(count_words(4537), "4,537 calibres");
    assert_eq!(count_words(1), "1 calibre");
    assert_eq!(shown_of(4537, 4537), "4,537 calibres");
    assert_eq!(shown_of(6, 4537), "6 of 4,537 calibres");
    let schild = cal.find("A. Schild 1017").unwrap();
    assert_eq!(cal.cell(schild, Col::LiftAngle), "51");
}

#[test]
fn the_maker_is_named_once() {
    assert_eq!(with_maker_name("Rolex", "Rolex 3235"), "Rolex 3235");
    assert_eq!(with_maker_name("Rolex", "rolex 3235"), "rolex 3235");
    assert_eq!(with_maker_name("ETA", "2824-2"), "ETA 2824-2");
    // A name that only begins with the same letters is a different word.
    assert_eq!(
        with_maker_name("Seiko", "SeikoMatic 6218"),
        "Seiko SeikoMatic 6218"
    );
    // Under another of the maker's names.
    assert_eq!(
        with_maker_name("Tianjin Seagull", "Seagull ST36"),
        "Seagull ST36"
    );
    // A clone stays marked as one.
    assert_eq!(
        with_maker_name("APS (clone)", "APS 4401"),
        "APS 4401 (clone)"
    );
    for name in ["Patek 240 Clone", "Patek 240 (clone)", "Patek 240 CLONE"] {
        assert_eq!(with_maker_name("(clone)", name), name);
    }
    assert_eq!(
        with_maker_name("Shanghai (clone)", "Patek 240 Clone"),
        "Shanghai Patek 240 Clone"
    );
    // Do not lose the clone marker when it is absent or only a substring.
    assert_eq!(with_maker_name("(clone)", "240"), "(clone) 240");
    assert_eq!(
        with_maker_name("(clone)", "Clonecraft 240"),
        "(clone) Clonecraft 240"
    );
}

#[test]
fn saved_picks_follow_the_tables_renames() {
    let c = calibres::all()
        .iter()
        .find(|c| !c.former_names.is_empty())
        .expect("a renamed calibre in the table");
    let (old, new) = (c.former_names[0].clone(), c.calibre.clone());
    let mut cal = Calibres::in_memory();
    assert_eq!(cal.current_name(&old), new);
    assert_eq!(cal.current_name("Not in the table"), "Not in the table");
    // Saved under both names: one row, and on the short list either way.
    cal.restore(
        vec![old.clone(), new.clone()],
        vec![old.clone()],
        vec![old.clone()],
    );
    assert_eq!(cal.recent, vec![new.clone()]);
    assert_eq!(cal.short_added, vec![new.clone()]);
    assert_eq!(cal.short_removed, vec![new.clone()]);
    assert!(cal.find(&new).is_some_and(|e| e.name() == new));
}

#[test]
fn no_row_in_the_table_names_its_maker_twice() {
    let mut twice = Vec::new();
    for c in calibres::all() {
        let row = with_maker_name(&c.maker, &c.calibre);
        let words: Vec<String> = row
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect();
        let repeated = maker_words(&c.maker).iter().any(|name| {
            name.split(|ch: char| !ch.is_alphanumeric())
                .filter(|w| w.len() > 2)
                .any(|w| words.iter().filter(|x| **x == w.to_lowercase()).count() > 1)
        });
        if repeated {
            twice.push(row);
        }
    }
    assert!(
        twice.is_empty(),
        "{} rows: {:?}",
        twice.len(),
        &twice[..twice.len().min(10)]
    );
}

fn store_fixture(label: &str) -> (PathBuf, PathBuf) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tg-calibre-protection-{label}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("calibres.json");
    (dir, path)
}

fn legacy_calibre() -> settings::CustomCalibre {
    settings::CustomCalibre {
        name: "Legacy Bench".into(),
        wheels: vec![settings::CustomWheel {
            name: " Third Wheel ".into(),
            period_s: 450.0,
        }],
    }
}

#[test]
fn unreadable_catalogue_is_never_replaced_by_fallback_edits() {
    for bytes in [
        b"{ invalid catalogue, preserve me".as_slice(),
        br#"{"schema":"timegrapher.user-calibres/future","custom":[],"wheels":[]}"#,
    ] {
        let (dir, path) = store_fixture("blocked");
        std::fs::write(&path, bytes).unwrap();
        let mut c = Calibres::open(Some(path.clone()));
        assert!(c.user.custom.is_empty());
        let problem = c.problem.clone().expect("load failure is visible");
        assert!(problem.contains(&path.display().to_string()));
        assert!(c.find("Rolex 4130").is_some(), "built-ins remain usable");
        c.new_custom("Temporary New");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        c.adopt("Temporary Imported", &[("third wheel".into(), 123.0)]);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        c.migrate(vec![legacy_calibre()]);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(c.pending_legacy(), &[legacy_calibre()]);
        // Wheel changes and custom editor updates share this save boundary.
        c.user.set_wheel("Rolex 4130", "third wheel", 400.0, None);
        c.user.remove_custom("Temporary New");
        c.save();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(c.problem.as_ref(), Some(&problem));
        assert!(problem.contains("only in memory"));
        assert!(!path.with_extension("json.tmp").exists());
        // Repairing externally does not authorize this empty-fallback
        // instance to overwrite a catalogue it has never loaded.
        let mut repaired = UserCalibres::default();
        repaired.put_custom(custom("Repaired Original", [("third wheel", 321.0)]));
        repaired.save(&path).unwrap();
        let repaired_bytes = std::fs::read(&path).unwrap();
        c.new_custom("Still Temporary");
        assert_eq!(std::fs::read(&path).unwrap(), repaired_bytes);
        let reopened = Calibres::open(Some(path));
        assert!(reopened.problem.is_none());
        assert!(reopened.find("Repaired Original").is_some());
        assert!(reopened.find("Still Temporary").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn unreadable_directory_catalogue_stays_blocked_after_path_repair() {
    let (dir, path) = store_fixture("directory");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("original"), b"keep").unwrap();
    let mut c = Calibres::open(Some(path.clone()));
    c.new_custom("Temporary");
    assert_eq!(std::fs::read(path.join("original")).unwrap(), b"keep");
    std::fs::remove_dir_all(&path).unwrap();
    c.new_custom("Still Temporary");
    assert!(
        !path.exists(),
        "a repaired path does not unblock this instance"
    );
    assert!(c.problem.as_ref().unwrap().contains("only in memory"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_settings_survive_failed_save_until_successful_retry() {
    let (dir, path) = store_fixture("retry");
    let mut c = Calibres::open(Some(path.clone()));
    // Deterministic rename failure, without permission assumptions.
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("original"), b"keep").unwrap();
    let old = vec![legacy_calibre()];
    c.migrate(old.clone());
    assert_eq!(c.pending_legacy(), old);
    assert!(c.find("Legacy Bench").is_some());
    assert!(c.problem.as_ref().unwrap().contains("couldn't be saved"));
    assert_eq!(std::fs::read(path.join("original")).unwrap(), b"keep");
    std::fs::remove_dir_all(&path).unwrap();
    // A normal later edit retries the save, including the migrated entry.
    c.adopt("Later Entry", &[("fourth wheel".into(), 60.0)]);
    assert!(c.pending_legacy().is_empty());
    assert!(c.problem.is_none());
    let reopened = Calibres::open(Some(path));
    assert!(reopened.find("Legacy Bench").is_some());
    assert!(reopened.find("Later Entry").is_some());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_valid_and_memory_only_catalogues_keep_their_contracts() {
    let (dir, path) = store_fixture("valid");
    let mut c = Calibres::open(Some(path.clone()));
    assert!(c.problem.is_none());
    c.new_custom("First Entry");
    let mut c = Calibres::open(Some(path.clone()));
    c.adopt("Second Entry", &[("third wheel".into(), 123.0)]);
    c.migrate(vec![legacy_calibre()]);
    assert!(c.pending_legacy().is_empty());
    let reopened = Calibres::open(Some(path));
    for name in ["First Entry", "Second Entry", "Legacy Bench"] {
        assert!(reopened.find(name).is_some(), "{name}");
    }
    let mut memory = Calibres::in_memory();
    memory.new_custom("Memory Entry");
    memory.migrate(vec![legacy_calibre()]);
    assert!(memory.find("Memory Entry").is_some());
    assert!(memory.find("Legacy Bench").is_some());
    assert_eq!(memory.pending_legacy(), &[legacy_calibre()]);
    assert!(memory.problem.is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

/// A calibre the search hides isn't shown in full beside the rows the
/// search found, and the count says how many of all are shown.
#[test]
fn a_hidden_calibre_leaves_the_details_and_the_count_says_of_how_many() {
    let mut c = Calibres::in_memory();
    c.selected = Some("A. Schild 1017".into());
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let draw = |c: &mut Calibres| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1440.0, 900.0),
            )),
            ..Default::default()
        };
        let mut out = None;
        for _ in 0..2 {
            out = Some(ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    c.contents(ui, "", "test");
                });
            }));
        }
        out.unwrap()
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let all = c.every().count();
    let seen = draw(&mut c);
    assert!(
        !seen.iter().any(|t| t.starts_with("Select a calibre")),
        "{seen:?}"
    );
    assert!(seen.contains(&count_words(all)), "{seen:?}");
    c.search = "2824".into();
    let found = c.row_names().len();
    assert!(found < all);
    let seen = draw(&mut c);
    assert!(
        seen.iter().any(|t| t.starts_with("Select a calibre")),
        "{seen:?}"
    );
    assert!(
        seen.contains(&format!("{found} of {}", count_words(all))),
        "{seen:?}"
    );
    // Still selected: clearing the search shows it again.
    assert_eq!(c.selected.as_deref(), Some("A. Schild 1017"));
}

#[test]
fn blocked_catalogue_warning_is_painted_without_a_selection() {
    let (dir, path) = store_fixture("warning");
    std::fs::write(&path, b"invalid JSON").unwrap();
    let mut c = Calibres::open(Some(path));
    assert!(c.selected.is_none());
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            c.manager_body(ui, "", "test", &mut Managed::default());
        });
    });
    let text = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("only in memory"), "{text}");
    assert!(text.contains("will not be overwritten"), "{text}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn settings_calibres_move_to_the_user_file_once() {
    let dir = std::env::temp_dir().join(format!("tg-calibres-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("calibres.json");
    let mut c = Calibres::open(Some(path.clone()));
    assert!(c.problem.is_none());
    let old = vec![settings::CustomCalibre {
        name: "Bench Watch".into(),
        wheels: vec![
            settings::CustomWheel {
                name: " Third Wheel ".into(),
                period_s: 450.0,
            },
            settings::CustomWheel {
                name: String::new(),
                period_s: 10.0,
            },
        ],
    }];
    c.migrate(old.clone());
    let again = Calibres::open(Some(path.clone()));
    assert_eq!(again.user.custom.len(), 1);
    assert_eq!(
        again.user.custom[0].wheels.len(),
        1,
        "the unnamed wheel is left out"
    );
    assert_eq!(again.user.custom[0].wheels[0].name, "third wheel");
    // Moving them again changes nothing.
    let mut again = again;
    again.migrate(old);
    assert_eq!(Calibres::open(Some(path)).user.custom.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Teeth and Leaves (#399): a period the teeth give fills a blank wheel
/// and is captioned From Teeth; one they disagree with keeps the typed
/// figure and says what the teeth give.
#[test]
fn counted_teeth_caption_the_wheels() {
    let mut c = Calibres::in_memory();
    c.user
        .set_count("ETA 2892-A2", "escape wheel", Some(21), None);
    c.user
        .set_count("ETA 2824-2", "fourth wheel", Some(100), None);
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let draw = |c: &mut Calibres, name: &str| {
        c.selected = Some(name.into());
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1440.0, 2000.0),
            )),
            ..Default::default()
        };
        let mut out = None;
        for _ in 0..2 {
            out = Some(ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    c.contents(ui, "", "test");
                });
            }));
        }
        out.unwrap()
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let seen = draw(&mut c, "ETA 2892-A2");
    assert!(seen.iter().any(|t| t == "From Teeth"), "{seen:?}");
    assert!(
        seen.iter().any(|t| t.ends_with("Teeth and Leaves")),
        "{seen:?}"
    );
    let slots = Catalog::new(&c.user).slots(c.find("ETA 2892-A2").unwrap());
    let esc = slots.iter().find(|s| s.name == "escape wheel").unwrap();
    assert_eq!(esc.period_s, Some(5.25));
    let seen = draw(&mut c, "ETA 2824-2");
    assert!(seen.iter().any(|t| t == "Teeth give 55.556 s"), "{seen:?}");
}

/// Every period box at 800 wide shows its value with no digit under its
/// right edge: whole up to six figures, a longer one with decimals
/// dropped (#1586). The Add a Wheel box too.
#[test]
fn a_period_box_shows_its_whole_value() {
    let mut c = Calibres::in_memory();
    c.user
        .set_wheel("ETA 2824-2", "third wheel", 999_999.999, None);
    c.user
        .set_wheel("ETA 2824-2", "fourth wheel", 9_999_999.999, None);
    c.selected = Some("ETA 2824-2".into());
    c.adding = Some(("moon wheel".into(), "21114.027".into()));
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(800.0, 2000.0),
        )),
        ..Default::default()
    };
    // A group row's control grows to its width over a few frames, each
    // one laid in the width the last one took, as the app repaints it.
    let mut out = None;
    for _ in 0..12 {
        out = Some(ctx.run(input.clone(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                c.contents(ui, "", "test");
            });
        }));
    }
    // A text box clips its text 1 px outside its inner edge; nothing else
    // here is clipped that narrow.
    let in_boxes: Vec<(String, f32)> = out
        .unwrap()
        .shapes
        .iter()
        .filter(|s| s.clip_rect.width() < 200.0)
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some((
                t.galley.text().to_string(),
                s.clip_rect.max.x - 1.0 - (t.pos.x + t.galley.rect.right()),
            )),
            _ => None,
        })
        .collect();
    // The barrel's box and the Add a Wheel box.
    let barrels = in_boxes.iter().filter(|(t, _)| t == "21114.027").count();
    assert!(barrels >= 2, "{in_boxes:?}");
    for want in ["999999.999", "10000000"] {
        assert!(
            in_boxes.iter().any(|(t, _)| t == want),
            "no box shows {want}: {in_boxes:?}"
        );
    }
    for (t, gap) in &in_boxes {
        assert!(*gap >= 0.0, "{t} cut by {} px", -gap);
    }
}

#[test]
fn wheels_typed_for_a_listed_calibre_name_its_cycles() {
    let mut c = Calibres::in_memory();
    let name = &calibres::common().next().unwrap().calibre;
    let before = c.wheels(name).unwrap().len();
    c.user.set_wheel(name, "third wheel", 450.0, None);
    let after = c.wheels(name).unwrap();
    assert!(after
        .iter()
        .any(|w| w.name == "third wheel" && w.period_s == 450.0));
    assert!(after.len() >= before);
    assert!(c.wheels("").is_none());
}

#[test]
fn the_short_list_starts_common_and_follows_the_ticks() {
    let mut c = Calibres::in_memory();
    let common = calibres::common().next().unwrap();
    let rare = calibres::all().iter().find(|x| !x.common).unwrap();
    assert!(c.in_short(common) && !c.in_short(rare));
    c.set_short(&common.calibre, false);
    c.set_short(&rare.calibre, true);
    assert!(!c.in_short(common) && c.in_short(rare));
    assert_eq!(c.short_removed, std::slice::from_ref(&common.calibre));
    assert_eq!(c.short_added, std::slice::from_ref(&rare.calibre));
    c.set_short(&common.calibre, true);
    c.set_short(&rare.calibre, true);
    assert!(c.short_removed.is_empty() && c.short_added.len() == 1);
    c.set_short("no such calibre", true);
    assert_eq!(c.short_added.len(), 1);
}

#[test]
fn recent_calibres_put_the_latest_first() {
    let mut c = Calibres::in_memory();
    for n in ["A", "B", "C", "A", "D", "E", "F", "G"] {
        c.remember(n);
    }
    assert_eq!(c.recent, ["G", "F", "E", "D", "A", "C"]);
    c.remember("");
    assert_eq!(c.recent.len(), RECENT);
}

#[test]
fn a_new_custom_calibre_takes_the_typed_name_when_free() {
    let mut c = Calibres::in_memory();
    assert_eq!(c.new_custom("Bench 1"), "Bench 1");
    assert_eq!(c.new_custom("Bench 1"), "My Calibre 2");
    let listed = &calibres::all()[0].calibre;
    assert_eq!(c.new_custom(listed), "My Calibre 3");
    // A custom calibre may be obscure or vintage: no wheel is assumed.
    assert!(c.wheels("Bench 1").unwrap().is_empty());
}

#[test]
fn a_custom_balance_period_follows_its_beat_rate() {
    let mut k = custom("Bench", []);
    let balance = |k: &CustomCalibre| {
        k.wheels
            .iter()
            .find(|w| w.name == "balance")
            .map(|w| w.period_s)
    };
    k.bph = Some(18000);
    follow_balance(&mut k, None);
    assert_eq!(balance(&k), Some(0.4));
    k.bph = Some(28800);
    follow_balance(&mut k, Some(18000));
    assert_eq!(balance(&k), Some(0.25));
    // A period typed by hand stays when the beat rate changes.
    k.wheels[0].period_s = 0.3;
    k.bph = Some(21600);
    follow_balance(&mut k, Some(28800));
    assert_eq!(balance(&k), Some(0.3));
    // No beat rate, no derived balance.
    let mut k = custom("Bench", []);
    k.bph = Some(36000);
    follow_balance(&mut k, None);
    k.bph = None;
    follow_balance(&mut k, Some(36000));
    assert_eq!(balance(&k), None);
}

/// A funnel's menu keeps its height whatever its search leaves listed:
/// every value, a few, or none.
#[test]
fn a_funnel_menu_keeps_its_height_as_you_search() {
    let mut c = Calibres::in_memory();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 900.0));
    let height = |c: &mut Calibres, typed: &str| {
        c.funnel_search = typed.to_string();
        let mut h = 0.0;
        for _ in 0..2 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(230.0);
                        let r = ui.scope(|ui| c.funnel_menu(ui, Col::Calibre, "Calibre"));
                        h = r.response.rect.height();
                    });
                },
            );
        }
        h
    };
    let all = height(&mut c, "");
    let few = c
        .funnel_values(Col::Calibre)
        .iter()
        .filter(|v| v.contains("2824"))
        .count();
    assert!(few > 0 && few < 9, "{few}");
    assert_eq!(height(&mut c, "2824"), all);
    assert_eq!(height(&mut c, "no such calibre"), all);
    assert!(all > 300.0, "{all}");
}

/// A funnel's search in the Calibre column also finds a calibre by one
/// of its other names, as the Calibre menu's does, and the Maker
/// column's lists only what it finds.
#[test]
fn a_funnel_search_finds_other_names() {
    let mut c = Calibres::in_memory();
    let (cal, other) = calibres::all()
        .iter()
        .find_map(|cal| {
            let name = Entry::Table(cal).name().to_lowercase();
            cal.family
                .iter()
                .find(|f| {
                    f.len() > 3
                        && f.chars().all(|ch| ch.is_alphanumeric() || ch == ' ')
                        && !name.contains(&f.to_lowercase())
                })
                .map(|f| (cal, f.clone()))
        })
        .expect("a calibre with another name its own doesn't hold");
    c.funnel_search = other.clone();
    let listed = c.funnel_listed(Col::Calibre);
    let name = c.cell(Entry::Table(cal), Col::Calibre);
    assert!(
        listed.contains(&name),
        "{other} doesn't list {name}: {listed:?}"
    );
    // Kept while the search stays; a new search lists afresh.
    assert_eq!(c.funnel_listed(Col::Calibre), listed);
    c.funnel_search = "no such calibre".into();
    assert!(c.funnel_listed(Col::Calibre).is_empty());
    c.funnel_search = "eta".into();
    let makers = c.funnel_listed(Col::Maker);
    assert!(makers.iter().any(|m| m == "ETA"), "{makers:?}");
    assert!(makers.len() < c.funnel_values(Col::Maker).len());
}

#[test]
fn a_column_filter_applies_only_on_ok() {
    let mut c = Calibres::in_memory();
    let offered = c.funnel_values(Col::BeatRate);
    assert!(offered.len() > 2);
    let one = offered[0].clone();
    // Untick every value but one, as in the menu, before OK.
    for v in &offered[1..] {
        c.draft_set(&offered, v, false);
    }
    assert!(
        c.funnels[Col::BeatRate as usize].is_none(),
        "nothing applied before OK"
    );
    let draft = c.funnel_draft.take();
    assert_eq!(draft.as_ref().map(|k| k.len()), Some(1));
    c.funnels[Col::BeatRate as usize] = draft;
    assert!(c.rows().iter().all(|e| c.cell(*e, Col::BeatRate) == one));
    // Ticking every value again is no filter at all.
    c.funnel_draft = c.funnels[Col::BeatRate as usize].clone();
    for v in &offered[1..] {
        c.draft_set(&offered, v, true);
    }
    assert!(c.funnel_draft.is_none());
}

#[test]
fn every_calibre_is_under_one_maker_common_makers_first() {
    let list = by_maker();
    let n: usize = list.iter().map(|m| m.2.len()).sum();
    assert_eq!(n, calibres::all().len());
    let first_rare = list.iter().position(|m| !m.1).unwrap_or(list.len());
    assert!(list[first_rare..].iter().all(|m| !m.1));
}

#[test]
fn titles_and_summaries_read_plainly() {
    assert_eq!(title_case("escape wheel"), "Escape Wheel");
    let e = Entry::Table(&calibres::all()[0]);
    assert!(summary(e).contains("bph") || summary(e).contains("beat rate not given"));
}

/// Manage Calibres' table: every calibre, the star column's funnel the
/// starred ones, and the custom ones under the maker Custom.
#[test]
fn the_manager_lists_every_calibre_and_the_short_list() {
    let mut cal = Calibres::in_memory();
    let names =
        |c: &Calibres| -> Vec<String> { c.rows().iter().map(|e| e.name().to_string()).collect() };
    assert_eq!(names(&cal).len(), calibres::all().len());
    cal.funnels[Col::Short as usize] = Some(["Starred".to_string()].into());
    let common = calibres::all().iter().filter(|c| cal.in_short(c)).count();
    assert_eq!(names(&cal).len(), common);
    assert!(common < calibres::all().len());
    cal.funnels = Default::default();
    let mine = cal.new_custom("Bench Special");
    assert_eq!(names(&cal).len(), calibres::all().len() + 1);
    let e = cal.find(&mine).unwrap();
    assert_eq!(cal.cell(e, Col::Maker), "Custom");
    cal.funnels[Col::Maker as usize] = Some(["Custom".to_string()].into());
    assert_eq!(names(&cal), vec![mine.clone()]);
    cal.funnels = Default::default();
    assert!(!cal.rows().is_empty());
}

/// Each column's funnel keeps the values ticked; the funnels combine,
/// each offers the values the others let through, and ticking every
/// value again is no filter.
#[test]
fn the_funnels_combine_and_offer_what_the_others_let_through() {
    let mut cal = Calibres::in_memory();
    let makers = cal.funnel_values(Col::Maker);
    assert!(makers.len() > 10 && makers.windows(2).all(|w| w[0] != w[1]));
    let maker = calibres::all()
        .iter()
        .find(|c| c.bph.is_some() && c.maker == "ETA")
        .map_or_else(|| calibres::all()[0].maker.clone(), |c| c.maker.clone());
    // One maker: the rows are that maker's, the Beat Rate funnel offers
    // only that maker's rates.
    for m in &makers {
        if *m != maker {
            cal.set_funnel(Col::Maker, m, false);
        }
    }
    assert_eq!(
        cal.funnels[Col::Maker as usize],
        Some([maker.clone()].into())
    );
    let theirs: Vec<&Calibre> = calibres::all()
        .iter()
        .filter(|c| c.maker == maker)
        .collect();
    assert_eq!(cal.rows().len(), theirs.len());
    let rates = cal.funnel_values(Col::BeatRate);
    for r in &rates {
        assert!(
            theirs
                .iter()
                .any(|c| cal.cell(Entry::Table(c), Col::BeatRate) == *r),
            "{r} is not a rate of {maker}"
        );
    }
    // The Maker funnel still offers every maker: its own filter doesn't
    // narrow what it lists.
    assert_eq!(cal.funnel_values(Col::Maker), makers);
    // A second funnel narrows further: both must match.
    let rate = rates[0].clone();
    for r in &rates[1..] {
        cal.set_funnel(Col::BeatRate, r, false);
    }
    let both = theirs
        .iter()
        .filter(|c| cal.cell(Entry::Table(c), Col::BeatRate) == rate)
        .count();
    assert_eq!(cal.rows().len(), both);
    assert!(cal
        .rows()
        .iter()
        .all(|e| cal.cell(*e, Col::Maker) == maker && cal.cell(*e, Col::BeatRate) == rate));
    // Ticking every rate offered again is no filter on Beat Rate.
    for r in &rates {
        cal.set_funnel(Col::BeatRate, r, true);
    }
    assert!(cal.funnels[Col::BeatRate as usize].is_none());
    assert_eq!(cal.rows().len(), theirs.len());
    // The star column's funnel combines with the others too.
    cal.funnels[Col::Short as usize] = Some(["Starred".to_string()].into());
    assert!(cal
        .rows()
        .iter()
        .all(|e| matches!(e, Entry::Table(c) if cal.in_short(c) && c.maker == maker)));
    // Unticking everything shows nothing; no funnel is every value.
    cal.funnels[Col::Short as usize] = None;
    cal.set_funnel(Col::Maker, &maker, false);
    assert!(cal.rows().is_empty());
    cal.funnels = Default::default();
    assert_eq!(cal.rows().len(), calibres::all().len());
}

/// Manage Calibres… from the Calibre menu with a rate known starts at
/// the calibres at that rate; Settings opened afresh starts unfiltered,
/// and a funnel the user changed is left alone.
#[test]
fn manage_calibres_starts_at_the_rate_known() {
    let mut cal = Calibres::in_memory();
    cal.open_manager("");
    cal.manager_rate = Some((28800, "the rate found from the beats"));
    cal.apply_rate_preset();
    let rows = cal.rows();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|e| e.bph() == Some(28800)));
    assert!(cal.rows().len() < calibres::all().len());
    // The gear: Settings opens the table unfiltered.
    cal.drop_rate_preset();
    assert_eq!(cal.rows().len(), calibres::all().len());
    // No rate known: nothing is set.
    cal.open_manager("");
    cal.apply_rate_preset();
    assert_eq!(cal.rows().len(), calibres::all().len());
    // A funnel changed after the preset stays as the user left it.
    cal.manager_rate = Some((28800, "the rate found from the beats"));
    cal.apply_rate_preset();
    cal.set_funnel(Col::BeatRate, "21600", true);
    cal.drop_rate_preset();
    assert!(cal.funnels[Col::BeatRate as usize].is_some());
}

/// The table's kept rows follow every change they depend on.
#[test]
fn the_kept_rows_follow_the_sort_the_filters_and_the_calibres() {
    let mut cal = Calibres::in_memory();
    let names = |c: &Calibres| -> Vec<(bool, String)> {
        c.rows()
            .iter()
            .map(|e| (e.is_custom(), e.name().to_string()))
            .collect()
    };
    assert_eq!(cal.row_names(), names(&cal));
    cal.sort = (Col::BeatRate, true);
    assert_eq!(cal.row_names(), names(&cal));
    cal.funnels[Col::Maker as usize] = Some(["ETA".to_string()].into());
    assert_eq!(cal.row_names(), names(&cal));
    cal.funnels[Col::Short as usize] = Some(["Starred".to_string()].into());
    assert_eq!(cal.row_names(), names(&cal));
    let first = cal.row_names()[0].1.clone();
    cal.set_short(&first, false);
    assert_eq!(cal.row_names(), names(&cal));
    cal.funnels = Default::default();
    let name = cal.new_custom("Kept Rows Test");
    assert_eq!(cal.row_names(), names(&cal));
    assert_eq!(cal.row_names()[0], (true, name));
}

/// The Calibre column leaves out the maker the Maker column already
/// shows, so rows of one maker differ in what is left: their numbers.
#[test]
fn the_calibre_column_drops_the_makers_name() {
    let cal = Calibres::in_memory();
    let rows = cal.rows();
    let schild = rows
        .iter()
        .find(|e| e.name() == "A. Schild 970")
        .expect("A. Schild 970 listed");
    assert_eq!(short_name(*schild), "970");
    for e in &rows {
        let short = short_name(*e);
        assert!(!short.trim().is_empty(), "{}", e.name());
        assert!(e.name().ends_with(short), "{} -> {short}", e.name());
    }
}

/// The custom calibres stay at the top whatever column the table is
/// sorted on, either way round; the rest follow the sort.
#[test]
fn custom_calibres_stay_at_the_top_whatever_the_sort() {
    let mut cal = Calibres::in_memory();
    let a = cal.new_custom("Zeta Bench");
    let b = cal.new_custom("Alpha Bench");
    for (col, ..) in COLS {
        for reverse in [false, true] {
            cal.sort = (col, reverse);
            let rows = cal.rows();
            let top: Vec<&str> = rows[..2].iter().map(|e| e.name()).collect();
            assert!(
                top.contains(&a.as_str()) && top.contains(&b.as_str()),
                "{col:?} {reverse}: {top:?}"
            );
            assert!(rows[2..].iter().all(|e| !e.is_custom()));
            // The listed ones in the column's order.
            for w in rows[2..].windows(2) {
                assert_ne!(
                    cal.order(&w[0], &w[1]),
                    std::cmp::Ordering::Greater,
                    "{col:?} {reverse}: {} before {}",
                    w[0].name(),
                    w[1].name()
                );
            }
        }
    }
    cal.sort = (Col::Calibre, false);
    let names: Vec<&str> = cal.rows()[..2].iter().map(|e| e.name()).collect();
    assert_eq!(names, [b.as_str(), a.as_str()]);
    cal.sort = (Col::Calibre, true);
    let names: Vec<&str> = cal.rows()[..2].iter().map(|e| e.name()).collect();
    assert_eq!(names, [a.as_str(), b.as_str()]);
    // A calibre with no beat rate comes after every rate, either way
    // round.
    for reverse in [false, true] {
        cal.sort = (Col::BeatRate, reverse);
        let rows = cal.rows();
        let first_blank = rows[2..].iter().position(|e| e.bph().is_none());
        if let Some(i) = first_blank {
            assert!(rows[2 + i..].iter().all(|e| e.bph().is_none()));
        }
        assert!(rows[2].bph().is_some());
    }
}

/// Enter in a funnel is OK: the values ticked become the column's filter,
/// as a click on OK makes them; without it nothing changes.
#[test]
fn enter_in_a_funnel_keeps_its_ticks() {
    let mut c = Calibres::in_memory();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 900.0));
    let run = |c: &mut Calibres, events: Vec<egui::Event>| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(230.0);
                    c.funnel_menu(ui, Col::Maker, "Maker");
                });
            },
        );
    };
    run(&mut c, vec![]);
    let one: std::collections::BTreeSet<String> =
        c.funnel_values(Col::Maker).into_iter().take(1).collect();
    c.funnel_draft = Some(one.clone());
    run(&mut c, vec![]);
    assert!(
        c.funnels[Col::Maker as usize].is_none(),
        "nothing kept before OK"
    );
    let enter = |pressed| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Default::default(),
    };
    run(&mut c, vec![enter(true), enter(false)]);
    let kept = c.funnels[Col::Maker as usize].clone().expect("Enter is OK");
    assert_eq!(kept, one);
}

/// A search typed in a funnel narrows what OK keeps to the values it
/// lists, as a spreadsheet's filter keeps its search results (Walter's
/// walk of #1192: "rol" then Enter left the table unfiltered).
#[test]
fn a_funnels_search_narrows_what_ok_keeps() {
    let mut c = Calibres::in_memory();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 900.0));
    let run = |c: &mut Calibres, events: Vec<egui::Event>| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(230.0);
                    c.funnel_menu(ui, Col::Maker, "Maker");
                });
            },
        );
    };
    run(&mut c, vec![]);
    run(&mut c, vec![egui::Event::Text("rol".into())]);
    let listed: std::collections::BTreeSet<String> =
        c.funnel_listed(Col::Maker).into_iter().collect();
    assert!(!listed.is_empty() && listed.len() < c.funnel_values(Col::Maker).len());
    let enter = |pressed| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Default::default(),
    };
    run(&mut c, vec![enter(true), enter(false)]);
    assert_eq!(c.funnels[Col::Maker as usize], Some(listed));
}
