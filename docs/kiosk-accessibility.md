# Kiosk accessibility notes

What the player kiosk does for players who cannot see or hear a warning, and the limits we accept.
The Help Centre (Epic 11) takes its wording from this page.

## Time warnings (Story 6.19)

- At 10, 5 and 1 minute left the HUD shows a solid warning toast at the top right: the title
  ("10 min left. Extend?"), the time the session ends, the grace line, **Dismiss** and the
  **Ctrl+Shift+E** key caps. It plays a two-note sound and never takes keyboard focus. It stays until
  dismissed or time-up; a newer warning replaces the older one.
- From 10 minutes left the time chip in the shell shows its warning border and a ⚠ glyph.
- The kiosk counts the marks on its own monotonic clock, so they still show with the Hub unreachable.

## Accepted limitation: fullscreen-exclusive games

A Direct3D fullscreen-exclusive game owns the screen, so no window can be drawn over it. At the
5- and 1-minute marks the kiosk draws nothing there: it flashes its taskbar button and plays the
warning sound.

**In true fullscreen-exclusive games the 5- and 1-minute warnings may therefore be sound-only for
deaf and hard-of-hearing players.** What still applies to them:

- the 10-minute chip warning (border and ⚠ glyph) in the shell, before the game takes the screen;
- the grace lock at time-up, which is drawn over the game when the game is borderless and always
  locks the PC when the grace ends;
- the **Ctrl+Shift+E** hotkey, which opens the HUD panel with the time left once the game is
  windowed or borderless.

Borderless-fullscreen games are not affected: the HUD draws over them like any window.
