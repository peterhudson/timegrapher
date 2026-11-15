//! [`Revision`]: what the take on show's readings rest on.

/// What the take on show's readings rest on, a counter for each part,
/// moved only where that part is set: every figure kept for later keys on
/// the parts it reads, so a change in one place reaches every pane. Moves
/// to core with #294's one measurement context per take.
///
/// Bumped in the put_* setters and in these places only: positions_moved
/// (breaks; also after the Settle field resettles the take) and
/// set_position (breaks), calibre_changed when the wheels change
/// (settings), and new_take from reset_session and show_log (every
/// part). Any other write to `rev` is a defect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Revision {
    /// The lift angle, the calibre and the beat rate.
    pub(super) settings: u64,
    /// The take's own clock and the clock kept for the input.
    pub(super) clock: u64,
    /// The take's breaks and its changes of position.
    pub(super) breaks: u64,
    /// The stretches masked out.
    pub(super) masks: u64,
    /// The takes opened and sessions begun: a pane's place is kept only
    /// while one stays open (Hugh on #1799, #1801).
    pub(super) opens: u64,
}

impl Revision {
    /// A take opened or a new session: every part is new.
    pub(super) fn new_take(&mut self) {
        self.settings += 1;
        self.clock += 1;
        self.breaks += 1;
        self.masks += 1;
        self.opens += 1;
    }
}
