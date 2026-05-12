//! Keyboard macros — record/replay sequences of [`EditorAction`]s.
//!
//! Slots are addressed by a single character (`a`–`z`). Each slot owns the
//! list of actions captured between [`MacroState::start_recording`] and
//! [`MacroState::stop_recording`]. Replay re-feeds the captured actions
//! through `AppState::update` inside an undo batch so the entire sequence
//! collapses to a single undo step.

use std::collections::HashMap;

use crate::input::action::EditorAction;

/// Per-process macro state. Lives on `AppState`.
#[derive(Default)]
pub struct MacroState {
    /// Currently-recording slot + accumulator. `None` when not recording.
    recording: Option<(char, Vec<EditorAction>)>,
    /// Stored slots from previous recordings.
    slots: HashMap<char, Vec<EditorAction>>,
    /// True while a recorded macro is being played back. Recording sees this
    /// flag and skips capturing the replayed actions (otherwise the same
    /// macro would grow on each play).
    replaying: bool,
    /// Most recently used slot — used as the default for unprompted replay.
    last_used: Option<char>,
}

impl MacroState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start recording into `slot`. Any previous content in the slot is
    /// discarded. Returns `false` if a recording is already in progress.
    pub fn start_recording(&mut self, slot: char) -> bool {
        if self.recording.is_some() {
            return false;
        }
        self.recording = Some((slot, Vec::new()));
        true
    }

    /// Stop the active recording (if any) and store its actions in the
    /// matching slot. Returns the slot character on success.
    pub fn stop_recording(&mut self) -> Option<char> {
        let (slot, actions) = self.recording.take()?;
        self.slots.insert(slot, actions);
        self.last_used = Some(slot);
        Some(slot)
    }

    /// Append an action to the in-progress recording if any. Ignored when
    /// not recording. Recordable actions are filtered by the caller.
    pub fn append(&mut self, action: &EditorAction) {
        if self.replaying {
            return;
        }
        if let Some((_, actions)) = &mut self.recording {
            actions.push(action.clone());
        }
    }

    /// Return a clone of the actions stored in `slot`, or `None` if the
    /// slot is empty.
    pub fn play(&mut self, slot: char) -> Option<Vec<EditorAction>> {
        let acts = self.slots.get(&slot)?.clone();
        self.last_used = Some(slot);
        Some(acts)
    }

    /// The currently-recording slot, if any. Used by the status bar.
    pub fn recording_slot(&self) -> Option<char> {
        self.recording.as_ref().map(|(c, _)| *c)
    }

    /// Default slot for `BeginReplayMacro` when invoked from a hotkey.
    #[allow(dead_code)]
    pub fn last_used_slot(&self) -> Option<char> {
        self.last_used
    }

    /// Acquire a guard that flips `replaying` to true for the duration of
    /// the borrow. Dropping the returned struct restores it. Used to
    /// suppress nested recording during playback.
    pub fn set_replaying(&mut self, replaying: bool) {
        self.replaying = replaying;
    }

    /// Whether a macro is currently playing back. Used by callers that need
    /// to skip work that should only happen on the original action stream.
    #[allow(dead_code)]
    pub fn is_replaying(&self) -> bool {
        self.replaying
    }
}

/// Return `true` when `action` should be captured into the current recording.
/// Excludes meta actions whose replay would be incoherent (mouse events,
/// the record/play controls themselves, and `Unhandled`).
pub fn is_recordable(action: &EditorAction) -> bool {
    !matches!(
        action,
        EditorAction::MouseClick { .. }
            | EditorAction::MouseDrag { .. }
            | EditorAction::MouseUp { .. }
            | EditorAction::MouseScroll { .. }
            | EditorAction::Unhandled
            | EditorAction::BeginRecordMacro
            | EditorAction::BeginReplayMacro
            | EditorAction::StopRecordMacro
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::action::Direction;

    #[test]
    fn record_and_play_round_trips() {
        let mut m = MacroState::new();
        assert!(m.start_recording('a'));
        m.append(&EditorAction::InsertChar('h'));
        m.append(&EditorAction::InsertChar('i'));
        m.append(&EditorAction::MoveCursor(Direction::Left));
        assert_eq!(m.stop_recording(), Some('a'));
        let played = m.play('a').unwrap();
        assert_eq!(played.len(), 3);
    }

    #[test]
    fn start_when_already_recording_is_rejected() {
        let mut m = MacroState::new();
        assert!(m.start_recording('a'));
        assert!(!m.start_recording('b'));
    }

    #[test]
    fn play_unknown_slot_returns_none() {
        let mut m = MacroState::new();
        assert!(m.play('z').is_none());
    }

    #[test]
    fn append_ignored_when_not_recording() {
        let mut m = MacroState::new();
        m.append(&EditorAction::InsertChar('x'));
        assert!(m.play('a').is_none());
    }

    #[test]
    fn append_skipped_during_replay() {
        let mut m = MacroState::new();
        m.start_recording('a');
        m.set_replaying(true);
        m.append(&EditorAction::InsertChar('x'));
        m.set_replaying(false);
        m.append(&EditorAction::InsertChar('y'));
        m.stop_recording();
        let played = m.play('a').unwrap();
        assert_eq!(played, vec![EditorAction::InsertChar('y')]);
    }

    #[test]
    fn is_recordable_filters_mouse_and_meta() {
        assert!(is_recordable(&EditorAction::InsertChar('a')));
        assert!(!is_recordable(&EditorAction::MouseClick { col: 0, row: 0 }));
        assert!(!is_recordable(&EditorAction::BeginRecordMacro));
        assert!(!is_recordable(&EditorAction::Unhandled));
    }

    #[test]
    fn last_used_tracks_most_recent_slot() {
        let mut m = MacroState::new();
        m.start_recording('a');
        m.stop_recording();
        assert_eq!(m.last_used_slot(), Some('a'));
        m.start_recording('b');
        m.stop_recording();
        assert_eq!(m.last_used_slot(), Some('b'));
        // Play also updates last-used.
        m.play('a');
        assert_eq!(m.last_used_slot(), Some('a'));
    }
}
