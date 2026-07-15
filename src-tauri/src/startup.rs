use std::ffi::OsStr;

pub const SILENT_START_ARG: &str = "--minimized";

pub fn is_silent_launch<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(SILENT_START_ARG))
}

#[cfg(test)]
mod tests {
    use super::{is_silent_launch, SILENT_START_ARG};

    #[test]
    fn detects_autostart_argument() {
        assert!(is_silent_launch(["Glance", SILENT_START_ARG]));
    }

    #[test]
    fn treats_normal_launch_as_visible() {
        assert!(!is_silent_launch(["Glance"]));
    }

    #[test]
    fn requires_an_exact_argument_match() {
        assert!(!is_silent_launch(["Glance", "--minimized=true"]));
    }
}
