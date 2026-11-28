//! The escapement a take is read for (#2313): the Swiss lever, or the
//! co-axial (in beta).
//!
//! On a Swiss lever the beat's last loud sound is the drop, so a beat
//! error can be fitted to the beat times and moved onto the unlocks
//! ([`crate::amplitude::unlock_beat_error_ms`]); where the unlock is not
//! found the drop's own beat error stands in. On a co-axial the two sides
//! end on different events (one impulse is given straight to the balance,
//! the other through the lever), so the drop's beat error measures no one
//! thing: the beat error is the unlock's or none. The unlock-to-drop time
//! is also shorter than on a lever at the same amplitude, so the lift
//! angle's amplitude gate says nothing about whether the edges were found
//! on the right sounds, and the unlock's beat error does not depend on it.
//! The amplitude is read with the lever's formula and is an estimate,
//! said so by [`crate::measure::Caveat::CoaxialEstimate`].

use serde::{Deserialize, Serialize};

/// The escapement a take is read for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Escapement {
    /// The Swiss lever: every reading as before #2313.
    #[default]
    #[serde(rename = "swiss lever")]
    SwissLever,
    /// The co-axial escapement (beta).
    #[serde(rename = "co-axial")]
    CoAxial,
}

impl Escapement {
    /// The escapement a calibre row's [`crate::calibres::Escapement::kind`]
    /// names, where Timegrapher reads it: `co-axial`. A pin lever or a
    /// detent is not read for (None), and its take is read as a lever's.
    pub fn of_kind(kind: &str) -> Option<Self> {
        (kind == "co-axial").then_some(Self::CoAxial)
    }

    /// The one rule for a take's escapement: the take's own setting
    /// (`take`, its session file's `escapement`), else its calibre's
    /// (`calibre`), else the Swiss lever.
    pub fn setting(take: Option<Self>, calibre: Option<Self>) -> Self {
        take.or(calibre).unwrap_or_default()
    }

    pub fn is_swiss_lever(&self) -> bool {
        *self == Self::SwissLever
    }

    pub fn is_co_axial(&self) -> bool {
        *self == Self::CoAxial
    }

    /// A reading's beat error from the drop, as this escapement gives it:
    /// `drop_ms` on a Swiss lever, none on a co-axial, whose two sides'
    /// last sounds are different events. The one rule every reading's
    /// drop beat error passes, so no surface falls back to it.
    pub fn drop_beat_error(self, drop_ms: Option<f64>) -> Option<f64> {
        drop_ms.filter(|_| self.is_swiss_lever())
    }

    /// The ratio of a window's two sides' unlock-to-drop times (Tick over
    /// Tock) its sides are judged against ([`crate::amplitude::sides_agree`],
    /// Norm's ruling on #1788), given the ratios `ratios` of the windows
    /// being screened: 1 on a Swiss lever, where one balance swings the
    /// same arc each way through the same lever, so the two times are
    /// equal; on a co-axial, whose two sides end on different events, the
    /// watch's own, the median of `ratios`, and none with fewer than
    /// [`crate::confidence::MIN_NOISE_VALUES`] of them, so no window is
    /// judged.
    pub fn sides_reference(self, ratios: &[f64]) -> Option<f64> {
        if self.is_swiss_lever() {
            return Some(1.0);
        }
        (ratios.len() >= crate::confidence::MIN_NOISE_VALUES)
            .then(|| crate::dsp::median(&mut ratios.to_vec()))
    }

    /// Whether a window whose two sides' edges were both outside the
    /// amplitude gate gives no unlock beat error: on a Swiss lever, where
    /// that gate says the edges were marked on the wrong sounds (#1977);
    /// not on a co-axial, where it says nothing about the edges.
    pub fn both_culled_unheard(self) -> bool {
        self.is_swiss_lever()
    }

    /// A whole-run fit `overall` as a document gives it: on a co-axial
    /// with no `beat_error_ms`, the beat times' and not a drop's (null).
    /// The one rule for `analyze`'s and `long`'s `overall`.
    pub fn overall_json(self, overall: Option<&crate::timing::TimingFit>) -> serde_json::Value {
        let mut v = serde_json::to_value(overall).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            if self.is_co_axial() {
                o.insert("beat_error_ms".into(), serde_json::Value::Null);
            }
        }
        v
    }

    /// The caveat a reading's beat error carries, where it carries one:
    /// on a Swiss lever, why it is the drop's where it is
    /// ([`crate::session::BeatErrorSource::caveat_why`]); on a co-axial
    /// whose readings are given (`read`), that it is the unlock's alone,
    /// or why there is none (`why`: the unlock's signs disagreeing,
    /// #1439; its sides disagreeing, #1788; else not found,
    /// [`crate::session::WindowFigures::drop_why`]). The one rule for every
    /// reading (Hugh's option (a) on #2317).
    pub fn beat_error_caveat(
        self,
        source: Option<crate::session::BeatErrorSource>,
        why: crate::measure::DropWhy,
        read: bool,
    ) -> Option<crate::measure::Caveat> {
        use crate::measure::Caveat;
        use crate::session::BeatErrorSource;
        if self.is_swiss_lever() {
            return source.and_then(|s| s.caveat_why(why));
        }
        let why = (source != Some(BeatErrorSource::Unlock)).then_some(why);
        read.then_some(Caveat::CoaxialBeatError { why })
    }

    /// The caveat a reading with an amplitude carries: on a co-axial, that
    /// it is an estimate (beta).
    pub fn amplitude_caveat(self, amplitude_shown: bool) -> Option<crate::measure::Caveat> {
        (self.is_co_axial() && amplitude_shown).then_some(crate::measure::Caveat::CoaxialEstimate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_takes_own_setting_outranks_its_calibres() {
        use Escapement::*;
        assert_eq!(Escapement::setting(None, None), SwissLever);
        assert_eq!(Escapement::setting(None, Some(CoAxial)), CoAxial);
        assert_eq!(
            Escapement::setting(Some(SwissLever), Some(CoAxial)),
            SwissLever
        );
        assert_eq!(Escapement::setting(Some(CoAxial), None), CoAxial);
    }

    #[test]
    fn only_the_co_axial_kind_is_read_for() {
        assert_eq!(Escapement::of_kind("co-axial"), Some(Escapement::CoAxial));
        for k in ["pin lever", "detent", ""] {
            assert_eq!(Escapement::of_kind(k), None, "{k}");
        }
        for k in crate::calibres::ESCAPEMENTS {
            assert_eq!(Escapement::of_kind(k).is_some(), k == "co-axial", "{k}");
        }
    }

    #[test]
    fn a_co_axial_gives_no_drop_beat_error() {
        assert_eq!(Escapement::SwissLever.drop_beat_error(Some(0.4)), Some(0.4));
        assert_eq!(Escapement::CoAxial.drop_beat_error(Some(0.4)), None);
    }

    #[test]
    fn a_co_axial_beat_error_says_where_it_comes_from() {
        use crate::measure::{Caveat, DropWhy};
        use crate::session::BeatErrorSource::{Drop, Unlock};
        let c = Escapement::CoAxial;
        let said = |why| Some(Caveat::CoaxialBeatError { why });
        let (false_, true_) = (DropWhy::UnlockNotFound, DropWhy::UnlockInconsistent);
        assert_eq!(c.beat_error_caveat(Some(Unlock), false_, true), said(None));
        assert_eq!(c.beat_error_caveat(Some(Unlock), true_, true), said(None));
        assert_eq!(
            c.beat_error_caveat(None, false_, true),
            said(Some(DropWhy::UnlockNotFound))
        );
        assert_eq!(
            c.beat_error_caveat(None, true_, true),
            said(Some(DropWhy::UnlockInconsistent))
        );
        // Its sides disagreeing in every window (#1788) is said too.
        assert_eq!(
            c.beat_error_caveat(None, DropWhy::SidesDisagree, true),
            said(Some(DropWhy::SidesDisagree))
        );
        // No readings given: nothing said of the beat error.
        assert_eq!(c.beat_error_caveat(None, false_, false), None);
        // A lever's is #1439's, unchanged.
        let l = Escapement::SwissLever;
        assert_eq!(l.beat_error_caveat(Some(Unlock), true_, true), None);
        assert_eq!(l.beat_error_caveat(None, false_, true), None);
        assert_eq!(
            l.beat_error_caveat(Some(Drop), true_, true),
            Some(Caveat::BeatErrorFromDrop {
                why: DropWhy::UnlockInconsistent
            })
        );
        assert_eq!(
            l.beat_error_caveat(Some(Drop), DropWhy::SidesDisagree, true),
            Some(Caveat::BeatErrorFromDrop {
                why: DropWhy::SidesDisagree
            })
        );
        assert_eq!(
            l.beat_error_caveat(Some(Drop), false_, false),
            Some(Caveat::FROM_DROP)
        );
    }

    #[test]
    fn the_setting_reads_as_the_calibre_table_spells_it() {
        #[derive(Deserialize)]
        struct S {
            escapement: Escapement,
        }
        let s: S = toml::from_str("escapement = \"co-axial\"").unwrap();
        assert_eq!(s.escapement, Escapement::CoAxial);
        let s: S = toml::from_str("escapement = \"swiss lever\"").unwrap();
        assert_eq!(s.escapement, Escapement::SwissLever);
        assert!(toml::from_str::<S>("escapement = \"detent\"").is_err());
    }
}
