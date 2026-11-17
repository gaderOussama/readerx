//! Folder watching. One thread per books folder: it waits for a debounced
//! change (or a timeout), runs a full rescan, and tells the screen.
//! The decisions are plain functions so they test without the OS.

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;
use std::{fs, thread};

/// Events are batched until the folder has been quiet this long.
const DEBOUNCE: Duration = Duration::from_millis(500);

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

enum Msg {
    Wake(Wake),
    Stop,
}

/// Handle to the watcher thread. Dropping it stops the thread.
pub struct Watcher(Sender<Msg>);

impl Drop for Watcher {
    fn drop(&mut self) {
        // the debouncer's callback holds a Sender too, so closing ours is not enough
        let _ = self.0.send(Msg::Stop);
    }
}

fn attach(folder: &Path, tx: &Sender<Msg>) -> Option<Debouncer<RecommendedWatcher>> {
    let tx = tx.clone();
    let mut d = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| {
        let _ = tx.send(Msg::Wake(if res.is_ok() { Wake::Event } else { Wake::Error }));
    })
    .ok()?;
    d.watcher().watch(folder, RecursiveMode::Recursive).ok()?;
    Some(d)
}

/// Watch `folder` on its own thread. `scan` runs a full rescan and returns
/// `(too_new, readable)`; `changed` tells the screen to refresh. Nothing here
/// touches the library: a vanished folder only changes what `scan` reports.
pub fn start(
    folder: PathBuf,
    mut scan: impl FnMut() -> (bool, bool) + Send + 'static,
    changed: impl Fn() + Send + 'static,
) -> Watcher {
    let (tx, rx) = mpsc::channel();
    let handle = Watcher(tx.clone());
    thread::spawn(move || {
        let mut attached = attach(&folder, &tx);
        // false when the attach failed, so the next tick retries it
        let mut readable = attached.is_some() && fs::read_dir(&folder).is_ok();
        let mut too_new = false;
        loop {
            let wake = match rx.recv_timeout(next_wait(too_new)) {
                Ok(Msg::Wake(w)) => w,
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => Wake::Tick,
            };
            let now_readable = fs::read_dir(&folder).is_ok();
            if !rescan_due(wake, too_new, readable, now_readable) {
                continue;
            }
            let (new_too_new, scan_readable) = scan();
            too_new = new_too_new;
            if !scan_readable {
                attached = None;
            } else if needs_attach(scan_readable, attached.is_some(), wake) {
                attached = attach(&folder, &tx);
            }
            readable = scan_readable && attached.is_some();
            changed();
        }
    });
    handle
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
