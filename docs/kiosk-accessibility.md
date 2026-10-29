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

## The grace at time-up (Story 6.22)

- When a prepaid session reaches zero and the branch has a grace period, the HUD shows a solid
  **Time's up** card at the top right: the grace countdown in large figures, "Wrap up your round.
  This PC locks when the timer hits 0:00.", and (for a player with a wallet) **Extend** with the
  **Ctrl+Shift+E** key caps. It plays the warning sound, never takes keyboard focus and covers only
  its own corner, so the game keeps every click and key elsewhere.
- Screen readers are told once when it appears, and again at 30 and 10 seconds left.
- The kiosk counts the grace on its own monotonic clock, so the card and the lock at its end still
  happen with the Hub unreachable.
- In a Direct3D fullscreen-exclusive game nothing can be drawn: the taskbar flashes and the warning
  sound plays once when the grace begins. As with the 5- and 1-minute warnings, this may be
  sound-only for deaf and hard-of-hearing players; the PC still locks when the grace ends.
- When the grace ends, "Clean up" mode ends the player's programs and returns to the attract screen.
  "Staff handles" mode shows "Time's up — see the counter." and **suspends** the player's programs
  (the game is frozen, not closed) until staff act: an Extend from the Hub thaws them and lifts the
  lock; End ends them as part of the session-end cleanup.

## The Games tab (Story 6.23)

- Every game is a real button (`game-tile`, 140 × 196, name on the tile scrim, or centred on
  `surface` with no cover). Tab moves through the tiles in name order; Enter or Space starts the
  game, and the focus ring is the 3px outline used across the shell.
- A running game's tile is named "<game>, running" for a screen reader and wears a "Running" chip.
  Activating it again brings the game's window forward instead of starting a second copy.
- A game that will not start raises an info toast on the HUD ("Couldn't start <game> — ask at the
  counter."); it uses the same polite live region as every other toast.
- While any game is running the shell stops all background video (the cover shows instead), so the
  game gets the PC's decode and GPU time.

## Filter and search the Games tab (Story 6.24)

- A `filter-rail` (a labelled `nav`) sits left of the grid: "All", then every category and tag in
  the library, once each, sorted. Each item is a button with `aria-pressed`; the active one is in
  `text`, bold, with a left bar (so it is not colour alone), the others in `text-muted`.
- Choosing an item filters the grid at once; there is no Apply button and one item is active at a
  time. A search field (`type="search"`, named "Search games") sits right-aligned in the title row
  and filters by name, ignoring case, together with the active item.
- Reading order is rail, search, grid. Tab visits every stop; Up and Down also step through the
  rail, and the arrow keys step through the grid. Every stop wears the 4px focus ring (3px on tiles).
- When nothing matches the grid is replaced by "No games match." and an underlined "Clear filters"
  button that resets to All with an empty search and puts focus in the search field.

## Launcher cards on the Games tab (Story 6.26)

- A row titled "Play on your own account" sits above the filter rail and the grid, as its own
  labelled section (heading plus a list). It shows a `launcher-card` for each of Steam, Epic Games
  and Riot Client that is installed on the PC, and is absent when none is.
- A card is one button on a `surface` card (radius lg, 280px wide, at least 88px tall). Its
  accessible name is its text: the launcher's name, "Use your own account", and "Open" while the
  launcher is open. The initial on the left is decorative (`aria-hidden`); state is words, not
  colour. Tab visits each card; Enter and Space open it; focus wears the 3px ring.
- Opening a launcher that is already open brings its window to the front. A launcher that will not
  open raises the same HUD toast as a game ("Couldn't start <name> — ask at the counter.").

## "Most played here" carousel on the Games tab (Story 6.27)

- A `most-played-carousel` sits above the filter rail and the grid, as its own labelled section:
  the heading "Most played here", the caption "Top 10 by play hours · last 7 days · <branch>" and
  a list of ranked game tiles in one row. It is absent when the last 7 days hold no plays, when
  none of the top games is installed on the PC, and when the Hub cannot be asked.
- A tile is the ordinary `game-tile` button. Its rank ("#1", "#2", ...) is a badge at the top-left
  (`bg` fill, `text` ink), written before the name in the page, so the tile's accessible name is its
  text: "#1 Name" (plus "Running" while the game runs, whose badge moves to the top-right). The
  ranks number the tiles shown: a top game that is not installed is skipped and the next one moves
  up. Tab visits each tile; Enter and Space launch the game; focus wears the 3px ring.
- Left and Right move focus from tile to tile and scroll the focused tile into view; the row also
  scrolls with the pointer. The keys stop at the first and the last tile.
