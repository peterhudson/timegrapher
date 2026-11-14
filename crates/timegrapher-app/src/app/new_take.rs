//! The kinds of take New Take starts, and the buttons that start them.

use super::guided;

/// The kinds of take New Take starts for the watch picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NewTake {
    Listen,
    Positions,
    Poise,
    Complication,
    PowerReserve,
}

impl NewTake {
    /// A test's name; Listen's is its button's.
    pub(super) fn name(self) -> &'static str {
        match self {
            NewTake::Listen => "Listen",
            NewTake::Positions => guided::Kind::Positions.name(),
            NewTake::Poise => guided::Kind::Poise.name(),
            NewTake::Complication => guided::Kind::Complication.name(),
            NewTake::PowerReserve => "Power Reserve Test",
        }
    }

    /// The guided test this is, if it is one.
    pub(super) fn guided(self) -> Option<guided::Kind> {
        match self {
            NewTake::Positions => Some(guided::Kind::Positions),
            NewTake::Poise => Some(guided::Kind::Poise),
            NewTake::Complication => Some(guided::Kind::Complication),
            _ => None,
        }
    }

    /// The take a guided test of `kind` is.
    pub(super) fn of(kind: guided::Kind) -> Self {
        match kind {
            guided::Kind::Positions => NewTake::Positions,
            guided::Kind::Poise => NewTake::Poise,
            guided::Kind::Complication => NewTake::Complication,
        }
    }
}

pub(super) const NEW_TAKES: [(NewTake, &str, &str, &str); 5] = [
    (
        NewTake::Listen,
        "Listen",
        "Listen to the watch in the position on the {card} card, for as long as you like; \
         Stop saves the take into the watch",
        "As long as you like",
    ),
    (
        NewTake::Positions,
        "Positions Test",
        "The six positions one after another, settled and measured in turn, saved into the \
         watch",
        "Six positions, guided",
    ),
    (
        NewTake::Poise,
        "Dynamic Poise Test",
        "The eight upright positions, 45° apart, settled and measured in turn: the rate's \
         once-round wave and the balance's heavy spot, saved into the watch",
        "Eight upright, 45° apart",
    ),
    (
        NewTake::Complication,
        "Complication Test",
        "One position with a complication off, on, then off again, settled and measured in \
         turn: how much it lowers the amplitude, saved into the watch",
        "Off, on, then off again",
    ),
    (
        NewTake::PowerReserve,
        "Power Reserve Test",
        "Wind the watch fully and listen in one position until it stops, saved into the watch",
        "From full wind to a stop",
    ),
];

/// New Take's captions, in [`NEW_TAKES`]' order: what each button's
/// caption under it says.
#[cfg(test)]
pub(crate) fn new_take_captions() -> [&'static str; NEW_TAKES.len()] {
    NEW_TAKES.map(|t| t.3)
}
