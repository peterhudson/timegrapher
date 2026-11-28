//! A window whose two sides' unlock-to-drop times disagree gives no
//! unlock beat error and no amplitude (Norm's ruling on #1788, #1707).

use super::*;
use crate::escapement::Escapement;

/// A window whose sides were `e` and `o` ms from the drop at their
/// unlocks, read at `deg` degrees each side.
fn win(i: usize, e: f64, o: f64, deg: f64) -> AmplitudeWindow {
    AmplitudeWindow {
        start_s: 2.0 * i as f64,
        end_s: 2.0 * i as f64 + 2.0,
        even_deg: Some(deg),
        odd_deg: Some(deg - 5.0),
        beat_error_ms: Some(0.3),
        beat_error_unlock_ms: unlock_beat_error_ms(Some(0.3), Some(-e), Some(-o)),
        even_unlock_ms: Some(-e),
        odd_unlock_ms: Some(-o),
        even_drop_ms: Some(0.0),
        odd_drop_ms: Some(0.0),
        extra: None,
        sound_1: [0.3; 2],
    }
}

/// A co-axial whose Tick's unlock-to-drop time is 1.5 of its Tock's by
/// design, with a stretch where the Tick's unlock is read on an earlier
/// sound (2.25, 1.5 of the design). Read for a co-axial, against the watch's own ratio,
/// every healthy window keeps its unlock beat error and the stretch
/// gives none; read against a lever's 1, the healthy ones would go.
fn coaxial(n: usize, misread: std::ops::Range<usize>) -> Vec<AmplitudeWindow> {
    (0..n)
        .map(|i| {
            let e = if misread.contains(&i) { 13.5 } else { 9.0 };
            win(i, e, 6.0, 280.0)
        })
        .collect()
}

#[test]
fn a_co_axial_is_judged_against_its_own_sides_ratio() {
    let mut w = coaxial(40, 20..24);
    screen_unlock_beat_error(&mut w, Escapement::CoAxial);
    for (i, w) in w.iter().enumerate() {
        let bad = (20..24).contains(&i);
        assert_eq!(w.sides_disagree(), bad, "{i}: {w:?}");
        assert_eq!(w.beat_error_unlock_ms.is_none(), bad, "{i}: {w:?}");
        assert_eq!(w.mean().is_none(), bad, "{i}: {w:?}");
    }
    let mut lever = coaxial(40, 20..24);
    screen_unlock_beat_error(&mut lever, Escapement::SwissLever);
    assert!(lever.iter().all(|w| w.beat_error_unlock_ms.is_none()));
}

/// Under 30 windows a co-axial has no reference of its own, so the screen
/// leaves its sides as they are (#1576's minimum).
#[test]
fn a_co_axial_with_too_few_windows_is_not_judged_on_its_sides() {
    let mut w = coaxial(29, 10..12);
    screen_unlock_beat_error(&mut w, Escapement::CoAxial);
    assert!(w.iter().all(|w| !w.sides_disagree() && w.mean().is_some()));
}

/// A window judged to disagree is judged afresh by the next screen: once
/// the co-axial's own ratio is known, its healthy windows' amplitudes come
/// back as read, and a lone outlier the side screen left out stays out.
#[test]
fn a_screen_run_again_puts_back_what_it_left_out() {
    let mut w = coaxial(40, 20..24);
    let as_read: Vec<[Option<f64>; 2]> = w.iter().map(|w| [w.even_deg, w.odd_deg]).collect();
    screen_unlock_beat_error(&mut w, Escapement::SwissLever);
    assert!(w.iter().all(|w| w.sides_disagree() && w.mean().is_none()));
    w[5].edit_extra(|x| x.outlier_deg[1] = Some(150.0));
    w[5].odd_deg = None;
    w[5].edit_extra(|x| x.sides_moved[1] = false);
    screen_unlock_beat_error(&mut w, Escapement::CoAxial);
    for (i, w) in w.iter().enumerate() {
        let bad = (20..24).contains(&i);
        assert_eq!(w.sides_disagree(), bad, "{i}");
        if !bad && i != 5 {
            assert_eq!([w.even_deg, w.odd_deg], as_read[i], "{i}");
            assert_eq!(w.outliers(), [None, None], "{i}");
        }
    }
    assert_eq!((w[5].even_deg, w[5].odd_deg), (as_read[5][0], None));
    assert_eq!(w[5].outliers(), [None, Some(150.0)]);
    // Screening again changes nothing.
    let once: Vec<_> = w
        .iter()
        .map(|w| (w.even_deg, w.odd_deg, w.extra()))
        .collect();
    screen_unlock_beat_error(&mut w, Escapement::CoAxial);
    let twice: Vec<_> = w
        .iter()
        .map(|w| (w.even_deg, w.odd_deg, w.extra()))
        .collect();
    assert_eq!(once, twice);
}

/// The long-template choice and this screen are one test at two limits:
/// with a reference of 1 and [`SIDES_UNLOCK_AGREE_SHARE`] it is
/// `sides_go_long`'s (#1703); the screen's [`SIDES_DISAGREE_RATIO`] as a
/// share passes exactly the times within that ratio of each other.
#[test]
fn the_sides_agree_within_the_share_of_their_mean() {
    let long = SIDES_UNLOCK_AGREE_SHARE;
    assert!(sides_agree(13.0, 14.9, 1.0, long));
    assert!(!sides_agree(13.0, 16.5, 1.0, long));
    // As sides_go_long takes them: distances before the beat, negative.
    assert_eq!(
        sides_agree(-0.013, -0.0149, 1.0, long),
        sides_agree(13.0, 14.9, 1.0, long)
    );
    let screen = share_of_ratio(SIDES_DISAGREE_RATIO);
    for (e, o, r0, agree) in [
        (13.0, 16.5, 1.0, true),
        (10.0, 13.9, 1.0, true),
        (10.0, 14.1, 1.0, false),
        (14.1, 10.0, 1.0, false),
        (9.0, 6.0, 1.5, true),
        (13.5, 6.0, 1.5, false),
        (9.0, 6.0, 1.0, false),
    ] {
        assert_eq!(sides_agree(e, o, r0, screen), agree, "{e} {o} {r0}");
    }
}

/// A log screened for a co-axial and then kept for a lever reads as one
/// screened for the lever: its sides judged again against 1 (Trevor 85 on
/// #2486).
#[test]
fn a_co_axial_log_kept_for_a_lever_reads_as_one_screened_for_it() {
    let mut kept = coaxial(40, 20..24);
    screen_unlock_beat_error(&mut kept, Escapement::CoAxial);
    kept_for(&mut kept, Escapement::SwissLever);
    let mut lever = coaxial(40, 20..24);
    screen_unlock_beat_error(&mut lever, Escapement::SwissLever);
    assert_eq!(format!("{kept:?}"), format!("{lever:?}"));
    // And back again.
    kept_for(&mut kept, Escapement::CoAxial);
    let mut co = coaxial(40, 20..24);
    screen_unlock_beat_error(&mut co, Escapement::CoAxial);
    assert_eq!(format!("{kept:?}"), format!("{co:?}"));
}

/// The reason a reading gives no unlock beat error is the sides only when
/// most windows that found both sides were withheld for them (Norm 32 on
/// #2486): `disagree` of 10 such windows withheld for their sides, the
/// rest outside the far fences.
fn why_of(disagree: usize) -> crate::measure::DropWhy {
    let mut w: Vec<AmplitudeWindow> = (0..10).map(|i| win(i, 7.0, 7.0, 280.0)).collect();
    for (i, w) in w.iter_mut().enumerate() {
        w.beat_error_unlock_ms = None;
        if i < disagree {
            w.edit_extra(|x| x.sides_disagree = true);
        }
    }
    let refs: Vec<&AmplitudeWindow> = w.iter().collect();
    crate::session::window_figures(&refs, false).drop_why()
}

#[test]
fn the_sides_name_the_reason_only_when_most_windows_were_withheld_for_them() {
    use crate::measure::DropWhy;
    assert_eq!(why_of(4), DropWhy::UnlockNotFound);
    assert_eq!(why_of(5), DropWhy::UnlockNotFound);
    assert_eq!(why_of(6), DropWhy::SidesDisagree);
    assert_eq!(why_of(10), DropWhy::SidesDisagree);
    assert_eq!(why_of(0), DropWhy::UnlockNotFound);
}

/// From 30 windows a co-axial has a reference of its own (#1576's
/// minimum, inclusive), so its misread windows are withheld (Trevor 85).
#[test]
fn a_co_axial_with_30_windows_is_judged_on_its_sides() {
    let mut w = coaxial(30, 10..12);
    screen_unlock_beat_error(&mut w, Escapement::CoAxial);
    for (i, w) in w.iter().enumerate() {
        assert_eq!(w.sides_disagree(), (10..12).contains(&i), "{i}");
    }
}

/// A co-axial's reference is the median ratio, which misreads short of
/// half the windows don't move: 22 healthy windows at 1.5 and 18 misread
/// at 4.5 have a mean of 2.85, against which the healthy ones would sit
/// 1.9 off and be withheld; against the median they keep their unlock
/// beat error (Trevor 85).
#[test]
fn a_co_axial_is_judged_against_the_median_ratio() {
    let mut w: Vec<AmplitudeWindow> = (0..40)
        .map(|i| {
            win(
                i,
                if i.is_multiple_of(2) && i < 36 {
                    27.0
                } else {
                    9.0
                },
                6.0,
                280.0,
            )
        })
        .collect();
    let misread = |i: usize| i.is_multiple_of(2) && i < 36;
    assert_eq!((0..40).filter(|&i| misread(i)).count(), 18);
    screen_unlock_beat_error(&mut w, Escapement::CoAxial);
    for (i, w) in w.iter().enumerate() {
        assert_eq!(w.sides_disagree(), misread(i), "{i}");
        assert_eq!(w.beat_error_unlock_ms.is_some(), !misread(i), "{i}");
    }
}

/// A side whose edges were culled still counts as found for the majority
/// that names the reason (Norm 32: "kept or culled"): of 10 windows with
/// both sides found, 4 with a side culled, 5 withheld for their sides is
/// not more than half, so the unlock was not found (Trevor 85).
#[test]
fn culled_sides_count_towards_the_majority_that_names_the_reason() {
    let mut w: Vec<AmplitudeWindow> = (0..10).map(|i| win(i, 7.0, 7.0, 280.0)).collect();
    for (i, w) in w.iter_mut().enumerate() {
        w.beat_error_unlock_ms = None;
        if i < 5 {
            w.edit_extra(|x| x.sides_disagree = true);
        }
        if i >= 6 {
            let e = [
                w.even_unlock_ms.take().unwrap(),
                w.even_drop_ms.take().unwrap(),
            ];
            w.even_deg = None;
            w.set_culled([Some(e), None]);
        }
    }
    assert!(w.iter().all(|w| w.found_both()));
    let refs: Vec<&AmplitudeWindow> = w.iter().collect();
    assert_eq!(
        crate::session::window_figures(&refs, false).drop_why(),
        crate::measure::DropWhy::UnlockNotFound
    );
}
