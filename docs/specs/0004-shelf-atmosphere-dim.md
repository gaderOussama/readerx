# 0004. Shelf atmosphere, theme toggle and Dim desktop

**Date**: 2026-11-20
**Status**: Proposed

## Summary

The shelf gets a calm, quiet presence: a greeting with reading counts, a line art open book whose page slowly turns, a gentle first load fade and a small lift on hover. A sun or moon button on the shelf switches between the Paper and Night themes. In the reader, a Dim button darkens the rest of the desktop behind ReaderX, so the page is the only bright thing on screen.

## Requirements

- **AC-1**: The shelf header has an icon button beside Change folder. In Paper it shows a moon (`Dark theme`), in Night a sun (`Light theme`). A click switches the page theme and saves it, like the reader's Paper and Night buttons.
- **AC-2**: Next to the Continue card (or alone when there is none) a panel shows a greeting by the hour (`Good morning`, `Good afternoon`, `Good evening`, `Good night`) and a summary such as `2 in progress · 14 on the shelf`. It also shows the line art open book. The panel hides below 1000 px wide.
- **AC-3**: The empty shelf states show the same line art book.
- **AC-4**: Cards fade and rise in on the first render only. On hover the cover lifts 3 px and its border turns the text color. Every animation stops under `prefers-reduced-motion`.
- **AC-5**: The reader bar has a `Dim` toggle (`aria-pressed`, inverted when on). When on, a dark see through window (black at 62% opacity, a 400 ms fade) covers the monitor that ReaderX is on, behind ReaderX. Clicks pass through it.
- **AC-6**: The choice is saved as `settings.dimDesktop` (default off). Dim applies only while a book is open. Back, Escape and closing the window remove it. Minimizing ReaderX hides the overlay, restoring or focusing ReaderX shows it again, and moving ReaderX to another monitor moves the overlay with it.
- **AC-7**: Everything stays monochrome (palette variables only). The overlay is plain black.

## Decision

- **Overlay**: a second Tauri window, label `dim`, loading the static `ui/dim.html` (no script). It is transparent, undecorated, skips the taskbar, does not take focus, ignores the cursor (`set_ignore_cursor_events(true)`), and is not always on top. After the overlay is shown, ReaderX takes the focus again, so ReaderX stays above it. The window state plugin skips it (`with_denylist(&["dim"])`). The overlay is destroyed when the main window closes.
- **Commands**: `set_dim_desktop(on)` saves the setting and applies it. `dim_desktop(on)` only applies it, and the reader uses it on open and on Back. Both reject with `dim_failed`.
- **Shelf art**: inline SVG built with a small `s()` helper in `ui/lib/dom.js` (`ui/lib/book-art.js`). The greeting and the counts are pure functions in `ui/lib/greeting.js` with node tests.
- **Rejected**: changing the real wallpaper or the system brightness (that touches system settings and isn't undone if the app crashes), and a full screen ReaderX mode (the user wants to keep their window size).

## Consequences

- A crash leaves no lasting change: the overlay is only a window and goes away with the process.
- The taskbar stays above the overlay (Windows keeps it on top), and that is fine.

## Build plan

1. Rust: setting, commands, overlay window glue, tests.
2. Screen: greeting and book art helpers with tests, the shelf hero, the theme toggle, the Dim button.
3. Verify in the real app: theme toggle, Dim on and off, minimize and restore, Back, close.
