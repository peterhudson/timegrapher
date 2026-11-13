//! The accepted position names come from the positions themselves, so a
//! new one is listed once it exists (#1492); each is one `parse` takes.

use timegrapher_core::session::Position;

#[test]
fn the_accepted_names_are_the_positions_own() {
    let text = Position::accepted_names();
    assert_eq!(
        text,
        "DU, DD, CD, CL, CU, CR, 1H30, 4H30, 7H30 or 10H30 (or CH, CB, 9H, 6H, 3H or 12H)"
    );
    for name in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if !name.is_empty() && name != "or" {
            assert!(Position::parse(name).is_some(), "{name}");
        }
    }
}

#[test]
fn the_names_one_by_one_match_the_text() {
    for name in Position::names() {
        assert!(Position::accepted_names().contains(name), "{name}");
        assert!(Position::parse(name).is_some(), "{name}");
    }
    assert_eq!(Position::names().len(), 16);
}
