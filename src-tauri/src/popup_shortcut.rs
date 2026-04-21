#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupShortcutAction {
    HideWindow,
    ShowWindowAndFocusInput,
}

pub fn decide_popup_shortcut_action(main_window_visible: bool) -> PopupShortcutAction {
    // 将快捷键行为决策提取成纯函数，便于单测并避免窗口逻辑分支散落在事件回调里。
    if main_window_visible {
        PopupShortcutAction::HideWindow
    } else {
        PopupShortcutAction::ShowWindowAndFocusInput
    }
}

#[cfg(test)]
mod tests {
    use super::{PopupShortcutAction, decide_popup_shortcut_action};

    #[test]
    fn hides_window_when_main_window_is_visible() {
        assert_eq!(
            decide_popup_shortcut_action(true),
            PopupShortcutAction::HideWindow
        );
    }

    #[test]
    fn shows_window_and_focuses_input_when_main_window_is_hidden() {
        assert_eq!(
            decide_popup_shortcut_action(false),
            PopupShortcutAction::ShowWindowAndFocusInput
        );
    }
}
