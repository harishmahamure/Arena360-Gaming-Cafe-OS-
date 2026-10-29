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

## The Apps tab (Story 6.28)

- The Apps tab is the Games tab structure for the library apps (Discord, a browser, music): a
  `filter-rail` (a labelled `nav`, "Filter apps") of the apps categories and tags, a search field
  named "Search apps", and a grid of the same `game-tile` buttons, in name order, under the heading
  "Apps". The region is named "Apps on this PC". It has no launcher cards and no carousel.
- Reading order, Tab and arrow-key movement, the focus rings and the "No apps match." line with its
  "Clear filters" button are those of the Games tab (Stories 6.23 and 6.24). Enter or Space on a tile
  opens the app; an app that is already open has its window brought to the front instead of a second
  copy, and its tile is named "<app>, running" with the "Running" chip.
- An open app is not a game running: it does not stop the background video.
- An app that will not start raises the same info toast as a game ("Couldn't start <app> — ask at
  the counter.").
- With no apps on the PC the tab shows "No games here yet — ask at the counter." and no rail or
  search.

## Home: the featured game and the featured row (Story 6.29)

- Home is a labelled region ("Featured game"). The hero's game video (muted, never audio) or cover
  sits behind the whole shell under the scrim; the copy block (eyebrow, the game's name as an `h2`,
  the meta line, Play now, Game details) is in the scrim's zone, the left 45% × bottom 50%, at most
  864px wide (720px at 1366). Every line of it is `text` ink, not `text-muted`. In the high-contrast
  theme the hero is solid `bg` with no video.
- Reading and Tab order: the top bar, the device name, the hero's Play now and Game details, the
  video control (bottom-right of the hero), then the featured row. The row is a labelled `nav`;
  Left and Right also move focus along it and skip tiles hidden at the current width.
- Game details is a non-modal dialog ("About <game>") that lists Genre, Age rating and Launcher
  ("Not listed" when the library has none). Focus moves to its Close button; Esc or Close closes it
  and returns focus to Game details. `aria-expanded` on Game details tells the state.
- The featured tile is named "<game>, featured". Its 3px magenta outline is drawn 3px out from the
  tile; its focus ring is drawn 9px out, so the two are never mistaken for each other.
- The video control is the shared 44px `video-control`. Pausing it holds the frame for the rest of
  the session, across a trip to another tab and back. Play now stops the video entirely (cover shows)
  from the click.
- The eyebrow reads "Most played at <branch> this week" only when the hero is the branch's most played
  game; with no plays it reads "Featured at <branch>".

## The Shop tab (Story 6.30)

- Shop is the fourth pill after Apps and exists only when the Hub says orders are enabled on the
  branch (the `kiosk_orders` licence flag). On a Free branch the item is not in the nav at all, so
  it is not in the Tab or arrow-key order; a Shop tab left open when the Hub turns orders off is
  replaced by Home. The nav's roving focus follows the items that exist.
- The tab is a labelled region ("Food and drinks") with the same filter rail, search field and
  keyboard movement as the Games tab: Up and Down in the rail, all four arrows across the Add buttons
  in the grid (a disabled Add is skipped, as it is skipped by Tab). The search field is named
  "Search food & drinks". "Nothing matches." comes with a "Clear filters" button that returns focus
  to the search field.
- A card is an article: photo (decorative, empty alt: the name is next to it), name as an `h3`, the
  bundle's "2 × Cola" components as a list ("Includes"), the price in tabular figures, then Add,
  whose accessible name is "Add <item>". The "POPULAR" badge sits on the photo's top-left and is read
  with the card.
- Out of stock dims the photo only. The name and the "Out of stock" tag stay in `text` ink (the tag
  is also readable text, not colour alone, with a dashed edge) and Add is a disabled button, so
  Tab and Enter skip it. A card flips state within 2 s of a change at the Hub; focus stays where it
  is when a card turns unavailable.
- With nothing available (or an empty menu) the region reads "Kitchen's closed right now." and the
  rail and search are not shown.
- Add puts the item in the cart (Story 6.31, below).

## The cart (Story 6.31)

- Add opens the cart as a right-side panel: a non-modal dialog named "Your order", addressed "To
  <PC> · <zone>". Focus moves to the panel; Esc or Close shuts it and returns focus to the Shop's
  "Cart · n" button, which opens it again with the cart kept. The rest of the shell stays reachable.
- A line is a list item: name and line amount, bundle components indented beneath ("4 × Cola" for two
  combos), then a stepper group labelled "<qty> <name>" with "One fewer <name>" and "One more <name>"
  buttons (touch-sized). The last step down removes the line; an empty cart reads "Cart's empty."
- A step up asks the Hub for the live stock first. A step past what is available is refused: the
  quantity stays and the reason is an `alert` under the line with a ⚠ ("Only 2 left.", "Out of
  stock"), so it is never colour alone. After Place order, a short line is marked the same way and
  the cart stays; the kiosk has no override.
- The total is `hud-numeric` in tabular figures with "Inclusive of applicable taxes" under it. The
  one payment choice is a checked radio, "Add to session bill", with "Payable at the counter when your
  session ends"; Place order is the primary button and reads "Placing your order…" while it waits.
- With the Hub link down (or an answer that never came) Place order is disabled and a status line
  reads "Counter is reconnecting — try again shortly."; the cart is kept. A confirmation after a
  placed order is a `status` ("Order placed. It's on your session bill.").

## Paying for the cart (Story 6.32)

- The payment choices are a radio group ("How to pay"). Which radios exist is the Hub's answer for
  this PC: "Add to session bill" on a running postpaid or block session; "Pay now from wallet" with
  the balance for a signed-in player; "Pay at counter" always. A guest never sees the wallet. With
  the session ended the group holds "Pay at counter" alone.
- A wallet that cannot cover the total is a disabled radio. The reason is text beside it ("Balance
  ₹10.00. Top up at counter."), a `status`, so it is not colour alone. The default choice moves to
  the first one that works (bill, wallet, counter) and keeps the player's own pick while it is
  offered.
- If a concurrent spend leaves too little, Place order is refused and the cart stays with an `alert`
  that carries the real balance ("Balance ₹40.00. Top up at counter.").
- The confirmation after each choice is a `status` that says what happened ("Paid from your wallet.
  Your order is on its way." / "Order placed. Pay at the counter.").
- A PC that owes the counter for an order shows "Pay at the counter" and "Unpaid ₹80.00" on the
  attract screen in place of the rate. Keys and taps open nothing until the counter settles it; the
  next attract view lifts the hold on its own, with no player action.

## Order status on the HUD (Story 6.33)

- An order raises info toasts on the HUD, never taking focus and going by themselves after 8 s:
  "Ordered: 3 items" when it commits, "Order's ready — on its way to you." or "Order's ready — grab
  it at the counter." when staff mark it ready (the branch's fulfilment setting decides which), and
  "<item> was cancelled — see the counter." when an unserved line is cancelled. Each stays in the
  messages bell with its time, worded the same way.
- While any ordered line is unserved the pill carries an "Ordered" tag after the time and balance.
  It is text, not colour alone, and it is part of the pill's accessible name ("…. Ordered, not
  served yet"). It clears when the last line is served.
- After the Hub was unreachable the kiosk shows each open order's latest status once; toasts missed
  meanwhile are not replayed.
