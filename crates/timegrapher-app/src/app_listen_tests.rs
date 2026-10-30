//! The Listen card's Stop and Listen button, which share one place,
//! and the Save menu beside it.

use super::tests::queued_capture;
use super::*;

/// While a take is heard, Save's Keep in a Watch items are greyed and
/// say why: a take goes into a watch once it ends, so a pick is never
/// a silent no-op.
#[test]
fn keep_in_a_watch_waits_for_the_take_to_end() {
    let base = std::env::temp_dir().join(format!("tg-app-keep-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let root = base.join("Timegrapher Data");
    let mut lib = library::Library::create(&root).unwrap();
    lib.add(&library::Watch::new("Pepsi")).unwrap();
    drop(lib);
    let mut app = TimegrapherApp::with_devices(Vec::new(), None);
    app.use_library(Some(root.clone()));
    app.quick_listen = true;
    app.session.listen();
    let info = app.info("", 48000, 16);
    let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
    r.write(&timegrapher_core::capture::Block {
        samples: vec![0.0; 48000 * 5],
        at: std::time::SystemTime::now(),
    })
    .unwrap();
    app.session.recorder = Some(r);
    let _tx = queued_capture(&mut app, &base);
    assert!(app.running() && app.session.allows(Action::Save));
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut time = 0.0;
    let mut pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
        time += 0.25;
        let input = egui::RawInput {
            time: Some(time),
            events,
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.save_menu(ui, 32.0));
        });
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_string(),
                    t.galley.rect.translate(t.pos.to_vec2()).center(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let click = |at: egui::Pos2| {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    };
    pass(&mut app, Vec::new());
    // The Save button at the panel's top left: its menu opens.
    pass(&mut app, click(egui::pos2(8.0 + 16.0, 8.0 + 16.0)));
    let drawn = pass(&mut app, Vec::new());
    let pepsi = drawn
        .iter()
        .find(|t| t.0 == "Pepsi")
        .unwrap_or_else(|| panic!("{drawn:?}"))
        .1;
    assert!(drawn.iter().any(|t| t.0 == "New Watch…"), "{drawn:?}");
    // Over the watch: greyed, it says why.
    let mut said = false;
    pass(&mut app, vec![egui::Event::PointerMoved(pepsi)]);
    for _ in 0..8 {
        let drawn = pass(&mut app, Vec::new());
        said |= drawn.iter().any(|t| t.0 == KEEP_AFTER_STOP);
    }
    assert!(said, "the hover says why");
    // A click on it keeps nothing and changes no watch.
    pass(&mut app, click(pepsi));
    pass(&mut app, Vec::new());
    assert!(app.watch_open.is_none() && app.keep_back.is_none());
    assert!(app.session.saved_to().is_none());
    app.capture = None;
    let _ = std::fs::remove_dir_all(&base);
}
