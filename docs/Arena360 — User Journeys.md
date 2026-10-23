# Arena 360 — User Journeys

Oct 22, 2026 · @Harish

## The ten journeys

Ten journeys cover a cafe's day, an owner's week and a partner's referral (revised 23 Oct 2026, UX); every screen in the PRD exists to serve one of them. Each journey below is drawn as a flow with the actor, the app and the system step, followed by the steps as a checklist of what must be true.

| # | Journey | Driven by | Apps | The moment it must not fail |
| --- | --- | --- | --- | --- |
| 1 | Owner onboarding | Owner | Site, Onboarding, Hub, Kiosk | Pairing code to a live Hub in one try |
| 2 | Walk-in postpaid session | Counter staff, player | Hub, Kiosk | Bill matches the clock to the minute |
| 3 | Prepaid player at the kiosk | Player | Kiosk, Hub | Time-up does not lose a game |
| 4 | Kiosk order to kitchen | Player, kitchen | Kiosk, Hub, Kitchen display | Order reaches the ticket printer |
| 5 | Shift close and variance | Counter staff, manager | Hub | Expected cash is right so variance is real |
| 6 | Owner changes a price remotely | Owner | Owner app, cloud, Hub | Price lands once, exactly, and the owner sees it did |
| 7 | Credit player | Player, staff, owner | Hub, WhatsApp, Owner app | Block at the limit, not after |
| 8 | Hub fails, standby promoted | Counter staff | Hub standby, Kiosk | Kiosks reconnect by themselves |
| 9 | Partner refers a cafe | Partner | Marketing site, Partner portal, WhatsApp | The referred cafe is attributed and the payout arrives |
| 10 | Console session | Player, counter staff | Pi, phone, Hub | Nobody overruns unnoticed |

Colour in the flows: green boxes are the one step the journey exists for, grey boxes are everything around it, and a dashed box is a branch the design must handle but hopes not to see.

## 1. Owner onboarding

An owner goes from the marketing site to a live branch in under an hour without talking to anyone; the only thing they type into the Hub is the pairing code.

&#91;embedded content: owner onboarding · 10 steps\]

The wizard is resumable at every step, and steps 6 to 8 can be skipped and finished later from the Hub.

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Sign up | Owner | Marketing site | Phone OTP verified; owner account exists with MFA pending |
| Create branch | Owner | Onboarding web | Tenant, first branch, currency, timezone, GST flag saved; tier chosen (Free needs no card) |
| Pairing code | Cloud | Onboarding web | 8-character single-use code, 15-minute expiry, shown large with Copy and a countdown; no QR (revised 23 Oct 2026, UX) |
| Install Hub | Owner | Counter PC or Android tablet | Owner types the code; installer asks for nothing else and a bad code retries in place; Free tier offers Android only (revised 23 Oct 2026, UX) |
| Hub paired | Hub, cloud | Hub | Hub holds a device token and a signed entitlement; onboarding page advances on the first heartbeat |
| Kiosks pair | Owner or staff | Each gaming PC | Kiosk app finds the Hub by mDNS and asks for the 6-digit code from the Hub screen; each PC appears on the floor view as unassigned |
| Groups and plans | Owner | Onboarding web or Hub | At least one group with one hourly rate; template or CSV import with dry-run |
| Invite staff | Owner | Onboarding web | At least one manager with a PIN; invitation by phone |
| Test session | Owner | Hub and one kiosk | A 2-minute session starts, the kiosk HUD counts down, the screen locks at zero |
| Go live | Owner | Onboarding web | Checklist: opening hours, backup location, ad consent on Free; Owner app dashboard unlocks |

Failure branches: an expired or mistyped code shows an inline error in the Hub installer, which retries in place (revised 23 Oct 2026, UX), and a "generate a new code" button on the web page; a kiosk that cannot see the Hub shows the Hub's IP to type manually.

## 2. Walk-in postpaid session

The default way a cafe is used: the player sits, plays, and pays at the counter when done. The Hub computes the bill from the clock, so staff never argue about minutes.

&#91;embedded content: walk-in postpaid session · 10 steps, 1 branch\]

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Player arrives | Player | Counter | Nothing yet; a returning player may say their phone number, a new one may stay a guest |
| Find player | Staff | Hub floor view → sheet | Phone lookup in one field; "Guest" is one tap; a guest can be converted to a profile at settlement |
| Start postpaid | Staff | Session sheet | Tap a free tile, the sheet shows the device group rate and any active dynamic rule; one tap starts |
| Kiosk unlocks | Hub → Kiosk | Gaming PC | Unlock within 2 s; HUD shows elapsed time and the rate |
| Plays, orders | Player | Kiosk | Orders from the kiosk (Basic and above) attach to the running session and print a KOT (revised 23 Oct 2026, UX) |
| Session ends | Player or staff | Kiosk HUD or Hub tile | Player taps "I'm done" on the HUD, or staff end from the tile; the PC locks and shows "Pay at counter" |
| Bill computed | Hub | Session sheet | Minutes × rate with the branch rounding unit and minimum, plus orders, plus GST if enabled; the sheet shows the breakdown |
| Settle | Staff | Session sheet | Any mix of cash, UPI by reference (last 4 digits, required), card reference, wallet; the sheet will not close with a remainder (revised 23 Oct 2026, UX) |
| Rest to credit | Staff | Session sheet | Only for a profiled player with credit enabled; blocked at the limit with a manager-PIN override, once per player per day (revised 23 Oct 2026, UX) |
| Receipt | Hub | Printer or WhatsApp | Sequential invoice number; WhatsApp receipt needs the player's consent |
| PC back to free | Kiosk | Gaming PC | Launchers signed out, saved credentials cleared, attract screen up; tile turns green |

Timing budget: start under 10 s, settle under 10 s, measured from the first tap on the tile.

## 3. Prepaid player at the kiosk

A player with a pack or wallet balance never needs the counter: login, pick a plan, play, and extend from the HUD. The kiosk only asks; the Hub decides.

&#91;embedded content: prepaid player at the kiosk · 10 steps, 1 branch\]

"More time added" returns the player to playing with the HUD updated. If the Hub refuses a plan for insufficient balance, the kiosk shows the balance and "Top up at counter" and stays on the plan picker.

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Attract screen | Kiosk | Gaming PC | Branding, game video, current rates and the active dynamic rule; no ads (revised 23 Oct 2026, UX) |
| Login | Player | Kiosk | Username and password only, in one screen (staff can also start from the Hub); three failures lock the kiosk login for 60 s and tell the Hub (revised 23 Oct 2026, UX) |
| Pick plan | Player | Kiosk | Shows only plans valid for this device's group; packs show hours left and expiry; wallet shows balance and the hourly rate |
| Hub approves | Hub | — | One transaction: debit wallet or pack, write session, append change log; refusal returns a typed reason |
| Plays | Player | Kiosk | HUD counts down; game grid and library available; orders allowed |
| Warnings | Kiosk | HUD | Toast and sound at 10, 5 and 1 minutes; the HUD turns amber at 10 |
| Extend? | Player | HUD or lock screen | One tap opens extend options, also during grace; the Hub prices them live; no extension past closing time (revised 23 Oct 2026, UX) |
| Time up | Kiosk | Full screen | Grace countdown (default 2 min) over the still-visible game so the player can save |
| Lock screen | Kiosk | Gaming PC | Per-branch time-up mode: "clean up" ends non-allow-listed processes, or "staff handles" holds the lock over the paused game for staff (revised 23 Oct 2026, UX) |
| Cleanup | Kiosk | Gaming PC | Steam, Epic, Riot, Discord and browser profiles signed out; credentials cleared; back to attract |

The test that matters: time-up during a ranked match must leave the match running through the grace period, and the next player must never see the previous player's accounts.

## 4. Kiosk order to kitchen

A player orders from the seat; the kitchen sees a ticket with the seat number; the cost lands on the session bill. Nobody shouts across the room. Basic tier and above; on Free, orders are taken at the counter POS only.

&#91;embedded content: kiosk order to kitchen · 9 steps\]

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Open menu | Player or staff | Kiosk HUD or Hub POS | Menu with images and categories; items out of stock are greyed with "out" |
| Build cart | Player | Kiosk | Stock is checked live against the Hub; a bundle shows its components and refuses if any component is short, with staff override at the POS only |
| Place order | Player | Kiosk | Order attaches to the running session; a player without a session pays at the counter instead |
| KOT prints | Hub | Kitchen printer | Within 3 s; ticket carries seat number, items, modifiers, order id; reprint from the Hub |
| Kitchen display | Kitchen | Any tablet on LAN | New orders appear at the top with elapsed time; tap advances new → preparing |
| Marked ready | Kitchen | Display | Tap advances to ready; the ticket row turns green on the display |
| HUD shows ready | Kiosk | HUD | Toast on the player's screen; staff see a ready badge on the device tile for runner delivery |
| Served | Staff or kitchen | Display or Hub | Stock decremented at served (configurable to decrement at order); waste recorded if the order is cancelled after preparing |
| Settled with bill | Hub | Session sheet | Order lines appear on the session bill with GST; a player can also pay an order immediately from the HUD by wallet |

Failure branches: printer offline shows a red badge on the Hub and the kitchen display still receives the order; a cancelled order after preparing needs a reason and lands in the waste report.

## 5. Shift close and cash variance

The Hub knows what should be in the drawer; closing is counting what is. Staff see a number, not an accusation, and only a variance over the branch threshold brings in a manager.

&#91;embedded content: shift close and variance · 8 steps, 1 branch\]

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Shift opened | Staff | Hub | Opening float entered and confirmed; shift bound to the staff login |
| Sales all day | Staff | Hub | Every settlement records method; cash sales kick the drawer and add to expected cash |
| Deposits, spend | Staff or manager | Hub | Cash deposits to safe or bank and petty-cash expenses reduce expected cash, each with a reference or receipt photo |
| Close shift | Staff | Hub shift sheet | A denomination counter or a single total; the sheet never shows expected cash until counted cash is entered |
| Variance shown | Hub | Shift sheet | Counted minus expected, signed (+ over, − short), with the breakdown one tap away (revised 23 Oct 2026, UX) |
| Within threshold | Hub | Shift sheet | Threshold per branch (default ₹100); close completes with no PIN |
| Over threshold | Manager | Shift sheet | Manager PIN and a reason from a list plus free text; both go in the audit log |
| Owner alerted | Cloud | Owner app | Web push and WhatsApp with branch, staff, amount and reason; shown on the Owner home under "needs attention" |
| Z-report prints | Hub | Printer, Owner app | Sales by type and method, expected vs counted, variance, waste, expenses, refunds; the same numbers the Owner app shows |
| Next shift opens | Staff | Hub | Closing cash becomes the next float unless a deposit was made |

The rule that makes this honest: nothing in the day can change expected cash except a recorded sale, deposit, expense or refund, and each of those is in the change log with an actor.

## 6. Owner changes a price remotely

The owner never writes to a branch directly. A change becomes a command, the Hub pulls and applies it through the same code path a kiosk uses, and the owner watches it go from pending to applied.

&#91;embedded content: remote price change · 8 steps, 1 branch\]

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Edit a price | Owner | Owner app | Edit a plan or rate; choose one branch or all; preview shows which branches will change |
| Command queued | Cloud | Command queue | One command row per target branch with a shared group id; status pending; MFA session required |
| Pending in app | Owner app | Commands screen | Each branch shows "Waiting for branch" with the time queued; nothing looks broken |
| Hub polls | Hub | — | Every 60 s on paid tiers; a premium Hub also receives a poke over its tunnel and polls at once |
| Hub applies | Hub | — | Runs `apply(price.set)` in one transaction; records the command id locally so a retry cannot apply it twice |
| Rejected | Hub | — | Validation fails (plan deleted, group missing): status rejected with a reason code and message |
| Change logged | Hub | Change log | The price row and the command id go to the change log and up to the mirror on the next upload |
| Result reported | Hub → Cloud | Command queue | Status applied or rejected with timestamp |
| Owner sees it | Owner app | Commands screen | Badge flips to applied, or shows the rejection reason with a retry button |

If the branch is offline the command stays pending for 7 days and then expires, so a stale price cannot land weeks later; the owner sees "expired" and can resend. Floor view and kiosk attract screens show the new rate within one minute of apply.

## 7. Credit player

Regulars play on credit; the Hub enforces the limit before every spend rather than after, reminds on a schedule, and makes collection a one-tap counter action.

&#91;embedded content: credit player · 8 steps, 1 branch\]

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Credit enabled | Manager | Hub player profile | Limit in rupees and a reminder cycle (default 3, 7, 14 days); manager permission required |
| Plays on credit | Player | Kiosk or counter | A session or order chooses credit as the payment method; the Hub shows remaining headroom |
| Limit checked | Hub | `apply` | Every credit spend checks outstanding + new amount ≤ limit inside the transaction; a block returns a typed reason the kiosk shows as "Credit limit reached, see counter"; a manager PIN can override once per day, logged |
| Ledger grows | Hub | Player ledger | Each credit spend is a `wallet_txns` row with reason credit; outstanding is aged 0–7, 8–14, 15+ |
| Reminders | Hub → Gupshup | WhatsApp, SMS fallback | Template message with amount and branch name on the cycle days; needs consent captured at sign-up; no reminder if paid |
| Overdue 15 d+ | Cloud | Owner app | Alert on the Owner home; credit for that player auto-pauses until paid unless the branch disables that rule |
| Player returns | Player | Counter | Staff look up by phone; outstanding shown first on the profile |
| Collect payment | Staff | Hub | Any payment method; partial allowed; receipt references the settled transactions |
| Balance drops | Hub | Ledger | Outstanding reduces; credit reopens automatically if it was paused |

Free tier: credit works, reminders do not (no WhatsApp sending on Free); staff see the overdue list on the Hub instead.

## 8. Hub fails, standby promoted

The main Hub PC dies during a Saturday rush. Sessions keep running on the kiosks, staff promote the standby with a PIN, and the kiosks find the new main Hub by themselves (revised 23 Oct 2026, UX).

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Main Hub dies | Hub | Counter PC | Kiosks show "Reconnecting…"; running sessions continue on the last known time; the player is never locked out |
| Kiosk while Hub unreachable | Kiosk | Gaming PC | New login, orders and Extend are disabled with "Counter is reconnecting — try again shortly." |
| Open standby | Counter staff | Standby PC | The standby Hub shows "Promote to main" |
| Promote | Counter staff, manager | Standby Hub | A manager's PIN entered inline; promotion completes under 10 s |
| Kiosks reconnect | Kiosk → Hub | Gaming PC | Kiosks find the new main Hub by themselves and sessions reconcile; no player notices |
| Promote fails | Manager | Standby Hub | A warning says data since the last sync may be lost; a manager can force promote with PIN; the forced promote is audited and the owner is alerted |
| Old main returns | Hub | Old counter PC | Split-brain lock: it shows "This PC is no longer the main Hub" with "Make this the standby" and never serves kiosks |

The rule that keeps the money right: only one main Hub ever serves kiosks.

## 9. Partner refers a cafe

A reseller or installer (Imran in the UX spine) applies, is approved by WhatsApp, and earns 10% of each referred cafe's subscription every month it stays active and paying. Partners get no kiosk branding; branch customisation stays logo-only (revised 23 Oct 2026, UX).

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| Apply | Partner | Marketing site, Pricing → Partners | Light form: name, phone (OTP), email, city, business name, type (reseller or installer), cafes served |
| Under review | Partner | Partner portal | Only the application status shows until approval |
| Approved | Arena 360 team | WhatsApp | Approval by WhatsApp only, with the login link to the partner portal (partners.arena360.in, separate login) |
| KYC | Partner | Partner portal | PAN, bank details, GSTIN optional; required before the first payout |
| Share link | Partner | Partner portal | Referral link and code shown; a cafe owner signs up through it |
| Cafe referred | Control plane | Partner portal | The tenant carries the `referral_code`; the cafe is listed as referred with 10% accruing monthly |
| Payout | Control plane | Bank, Partner portal | Bank transfer by the 10th for the previous month; TDS at 2% (subject to tax advice) shown on each payout; GST-registered partners upload a GST invoice |

Failure branches: a balance under ₹500 shows as pending and rolls over; missing KYC holds the payout with "KYC needed before first payout"; a GST-registered partner without an uploaded invoice is held with "Upload GST invoice".

## 10. Console session

Two friends play on a PS5 at TV 3. With the Pi (v1.1) they request from a phone with no login and pay at the counter; without a Pi, staff start from the tile and the tile alone carries the warnings and time-up (revised 23 Oct 2026, UX).

| Step | Actor | Where | What must be true |
| --- | --- | --- | --- |
| TV idle | Pi | TV | Branch logo, TV number, console rates and a large "Scan to start" QR; no ads |
| Scan and request | Player | Phone | Hub page over the cafe LAN with no login; pick a console plan and tap Request; "Waiting for the counter — pay at the counter to start." |
| Request on Hub | Hub | Floor view tile | The console tile raises a "REQUEST" badge with sound; the requested plan is pre-selected in the sheet |
| Pay and start | Counter staff | Session sheet | Player pays at the counter or is looked up (or Guest); postpaid, pack and pass rules apply; Start switches the TV to the console within 3 s |
| Live timer | Hub → phone | Phone | The phone shows time left only, over the LAN, with "Extend at the counter"; a WhatsApp/SMS at 5 min goes out only with consent |
| Warnings | Pi | TV | At 5 and 1 min the TV switches to the Pi input for 5 s, then back to the game; the tile shows the warning states |
| Time-up | Pi, Hub | TV, floor view | No grace: the TV locks at once with "Time's up — see the counter."; CEC standby or smart plug per branch; the tile raises "TIME UP" with sound until acknowledged |
| End and settle | Counter staff | Session sheet | Staff end and settle; Undo until a receipt prints |
| Controller check | Counter staff | Floor view tile | Staff tap Returned or Missing; logged against the session; TV returns to idle |

Failure branches: extend is done at the counter and the phone and TV pick up the new time; a refused request shows "Ask at the counter." on the phone; a phone that leaves the cafe Wi-Fi keeps its last value with "Reconnect to cafe Wi-Fi to update".
