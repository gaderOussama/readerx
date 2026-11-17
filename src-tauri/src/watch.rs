//! Folder watching. One thread per books folder: it waits for a debounced
//! change (or a timeout), runs a full rescan, and tells the screen.
//! The decisions are plain functions so they test without the OS.

use std::time::Duration;

/// A file that was too new to read gets one more scan after this.
pub const RETRY: Duration = Duration::from_secs(3);
/// While nothing else happens, check this often that the folder is still there
/// (and, while it is missing, whether it came back).
pub const POLL: Duration = Duration::from_secs(10);

/// Why the watcher thread woke up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wake {
    /// A settled batch of file events.
    Event,
    /// The OS watcher reported an error or overflow.
    Error,
    /// The wait ran out with no message.
    Tick,
}

/// How long to wait for the next message. One wait, so timers never pile up.
pub fn next_wait(too_new: bool) -> Duration {
    if too_new {
        RETRY
    } else {
        POLL
    }
}

/// Events and errors always rescan. A tick rescans only when something is due:
/// a file was too new last time, or the folder went away or came back since.
pub fn rescan_due(wake: Wake, too_new: bool, was_readable: bool, readable_now: bool) -> bool {
    match wake {
        Wake::Event | Wake::Error => true,
        Wake::Tick => too_new || was_readable != readable_now,
    }
}

/// The OS watcher is only useful while the folder is readable. A new one is
/// needed when none is attached, and after an error (the old one may be dead).
pub fn needs_attach(readable: bool, attached: bool, wake: Wake) -> bool {
    readable && (!attached || wake == Wake::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_new_file_waits_3s_otherwise_polls_every_10s() {
        assert_eq!(next_wait(true), Duration::from_secs(3));
        assert_eq!(next_wait(false), Duration::from_secs(10));
    }

    #[test]
    fn events_and_errors_always_rescan() {
        for wake in [Wake::Event, Wake::Error] {
            assert!(rescan_due(wake, false, true, true));
            assert!(rescan_due(wake, false, false, false));
        }
    }

    #[test]
    fn quiet_tick_does_nothing_while_the_folder_is_unchanged() {
        assert!(!rescan_due(Wake::Tick, false, true, true));
        // still missing: no rescan, so no pointless refresh every 10 s
        assert!(!rescan_due(Wake::Tick, false, false, false));
    }

    #[test]
    fn tick_rescans_when_the_folder_vanished_or_returned() {
        assert!(rescan_due(Wake::Tick, false, true, false));
        assert!(rescan_due(Wake::Tick, false, false, true));
    }

    #[test]
    fn tick_rescans_after_a_too_new_file() {
        assert!(rescan_due(Wake::Tick, true, true, true));
    }

    #[test]
    fn attach_only_when_readable_and_not_attached_or_after_an_error() {
        assert!(needs_attach(true, false, Wake::Tick));
        assert!(needs_attach(true, true, Wake::Error));
        assert!(!needs_attach(true, true, Wake::Event));
        assert!(!needs_attach(false, false, Wake::Event));
        assert!(!needs_attach(false, true, Wake::Error));
    }
}
