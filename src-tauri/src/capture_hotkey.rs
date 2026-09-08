#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureHotkeyAction {
    Start,
    Cancel,
}

pub fn decide_capture_hotkey_action(capture_in_progress: bool) -> CaptureHotkeyAction {
    if capture_in_progress {
        CaptureHotkeyAction::Cancel
    } else {
        CaptureHotkeyAction::Start
    }
}

#[cfg(test)]
mod tests {
    use super::{decide_capture_hotkey_action, CaptureHotkeyAction};

    #[test]
    fn cancels_when_capture_is_already_running() {
        assert_eq!(
            decide_capture_hotkey_action(true),
            CaptureHotkeyAction::Cancel
        );
    }

    #[test]
    fn starts_when_no_capture_is_running() {
        assert_eq!(
            decide_capture_hotkey_action(false),
            CaptureHotkeyAction::Start
        );
    }
}
