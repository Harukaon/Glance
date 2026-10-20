/// What the global-shortcut manager should do with a stored binding.
///
/// The enable switch is a user-level "park this shortcut" control: it decides
/// whether a binding is live, but it never owns the combo string itself, so no
/// arm of this enum is allowed to clear or rewrite it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Register,
    Unregister,
    Noop,
}

pub fn decide_hotkey_action(combo: &str, enabled: bool) -> HotkeyAction {
    // Keep "not set" and "switched off" apart: an empty combo has nothing to
    // take down, while a disabled one still has a binding that must stop firing.
    if combo.is_empty() {
        HotkeyAction::Noop
    } else if enabled {
        HotkeyAction::Register
    } else {
        HotkeyAction::Unregister
    }
}

#[cfg(test)]
mod tests {
    use super::{decide_hotkey_action, HotkeyAction};

    #[test]
    fn registers_a_bound_enabled_combo() {
        assert_eq!(
            decide_hotkey_action("CommandOrControl+Shift+X", true),
            HotkeyAction::Register
        );
    }

    #[test]
    fn unregisters_a_bound_but_disabled_combo() {
        assert_eq!(
            decide_hotkey_action("CommandOrControl+Shift+X", false),
            HotkeyAction::Unregister
        );
    }

    #[test]
    fn ignores_an_unset_combo_even_when_the_switch_is_on() {
        assert_eq!(decide_hotkey_action("", true), HotkeyAction::Noop);
    }

    #[test]
    fn ignores_an_unset_combo_whose_switch_is_off() {
        assert_eq!(decide_hotkey_action("", false), HotkeyAction::Noop);
    }
}
