//! The calibre menu's search, drawn headless.

use super::*;

/// Shapes' words, drawn by one frame.
fn texts(out: &egui::FullOutput) -> Vec<String> {
    let mut t = Vec::new();
    for s in &out.shapes {
        if let egui::Shape::Text(x) = &s.shape {
            t.push(x.galley.text().to_string());
        }
    }
    t
}

#[test]
fn the_calibre_menu_searches_every_calibre() {
    let mut c = Calibres::in_memory();
    // One the short list doesn't hold, so only a search finds it.
    let rare = calibres::all().iter().find(|x| !x.common).unwrap();
    assert!(!c.in_short(rare));
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 800.0));
    let frame = |c: &mut Calibres, events: Vec<egui::Event>| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(300.0);
                    c.picker(ui, "", None);
                });
            },
        )
    };
    frame(&mut c, vec![]);
    // The menu's button, at the top of the panel.
    let at = egui::pos2(60.0, 18.0);
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(&mut c, vec![egui::Event::PointerMoved(at), press(true)]);
    frame(&mut c, vec![press(false)]);
    let open = frame(&mut c, vec![]);
    assert!(
        texts(&open).iter().any(|t| t == "Search every calibre"),
        "{:?}",
        texts(&open)
    );
    frame(&mut c, vec![egui::Event::Text(rare.calibre.clone())]);
    let found = frame(&mut c, vec![]);
    let want = format!("{} · {}", rare.calibre, rare.maker);
    assert!(
        texts(&found).contains(&want),
        "{want} not in {:?}",
        texts(&found)
    );
}

#[test]
fn manage_calibres_stays_in_view_under_a_long_short_list() {
    let mut c = Calibres::in_memory();
    for x in calibres::all() {
        c.set_short(&x.calibre, true);
    }
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 600.0));
    let frame = |c: &mut Calibres, events: Vec<egui::Event>| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(300.0);
                    c.picker(ui, "", None);
                });
            },
        )
    };
    frame(&mut c, vec![]);
    let at = egui::pos2(60.0, 18.0);
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(&mut c, vec![egui::Event::PointerMoved(at), press(true)]);
    frame(&mut c, vec![press(false)]);
    for _ in 0..10 {
        frame(&mut c, vec![]);
    }
    let open = frame(&mut c, vec![]);
    // Drawn, and inside what its clip lets the eye see.
    let seen = open.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(x) if x.galley.text() == "Manage Calibres…" => {
            let r = x.visual_bounding_rect();
            s.clip_rect.contains_rect(r) && screen.contains_rect(r)
        }
        _ => false,
    });
    assert!(seen, "Manage Calibres… scrolled out of view");
}
