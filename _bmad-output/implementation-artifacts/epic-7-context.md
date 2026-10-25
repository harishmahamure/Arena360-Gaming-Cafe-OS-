# Epic 7 Context: Kitchen tickets and display

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a Basic-and-above café run its kitchen from the Hub: kitchen-flagged order lines open a ticket, the ticket moves New, Preparing, Ready, Served, prints a KOT and shows on a paired kitchen display. Free branches see the kitchen as locked, and a lapse never ends work already running.

## Stories

- Story 7.1: Kitchen settings at the Hub, locked on Free
- Story 7.2: Kitchen notes on order lines from the POS and the kiosk cart
- Stories 7.3 to 7.11: ticket creation, KOT printing, display pairing, board, advance, LAN-loss sync, reference-hardware proof (see `epics.md`)

## Requirements and constraints

- **Tier gate (AD-29, AD-30):** the `kitchen` flag (Basic and above) gates new kitchen work. Story 7.1 builds the gate in `apply()`'s permission step: an intent registered as kitchen-gated is rejected `TIER_LOCKED` when the branch lacks the flag, and nothing is written. Registration is one line per intent in `arena-proto`. An intent registered as "existing work" passes after a lapse so open tickets finish. Each later story registers its own new kitchen intents and adds its own Free-refusal criterion.
- **Flag source:** until the licence stories (2.12, Epic 10) land, the Hub holds the verified licence's flags in the system-managed setting `licence.flags` (comma list). 2.12 writes it from the verified licence; nothing else does.
- **Upgrade link:** the Owner app Subscription screen does not exist until Epic 10, so every `tier-lock` shows "Available on Basic" with no "Upgrade" link.
- **Ticket model:** each placement with kitchen-flagged lines opens one ticket holding only those lines; the stage is the single source for the Hub sheet, the KOT, the display and the READY badge.
- **Display transport:** the Hub `:7401` listener serves the board over plain HTTP at `/kitchen` with a scoped `kds` token that can never cancel.
- **Seat only:** tickets identify the order by seat label and ticket number; no player name.
- **Printers:** the KOT printer is the receipt printer (default), a second ESC/POS printer with its own paper width, or none. The Story 2.27 find, manual IP and test-print flow serves both.
- **Every story** adds its new intents and their `.reverted` twins to the Story 1.8 `proptest` generator.
