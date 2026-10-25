# Epic 5 context: POS, inventory, tax-compliant receipts and shift close

Compiled from `epics.md` (Epic 5 notes) for the stories in this epic.

- Builds on the Epic 3 settle path (`orders`, `order_lines`, `payments`, `settle-bar`, `payment-split`, `bill.settled{order_id}` with `settled_at` and `undo_until` = settled_at + 8 s), the Story 3.14 irreversible-effect guard, the Story 2.27 printer adapter, the Story 2.13 `lock_branch_code()` function and the Story 3.25 alert service.
- **Invoice timing:** Print receipt issues the invoice number inside the settle intent (no Undo; correct by Refund); otherwise a system `invoice.issue` assigns it when the 8 s Undo window closes. Invoice number, receipt print and drawer kick are irreversible effects.
- **Cash:** from Story 5.10 a cash payment row needs an open shift (`SHIFT_NOT_OPEN`) and writes `cash_events`.
- **Tax:** Epic 3 bills were tax-inclusive with no tax line; from Story 5.2 every bill carries a per-rate breakup. GST logic applies only when `branch.tax_mode = 'gst_in'`; other branches use a simple tax with the same invoice series shape. Wallet top-ups and credit collections get a payment receipt, never an invoice number.
- **Out of the epic:** KOT and kitchen display (Epic 7), kiosk Shop and HUD "ordered" (Epic 6), Bluetooth printing (Epic 4), sending WhatsApp receipts and owner alerts (Epic 8), yearly GST archive and orphan reconciliation (Epics 8 to 9), player erasure (Epic 9).
- Every story adds its intents and `.reverted` twins to the Story 1.8 `proptest` generator.
- Reports screen is created by Story 5.27; later report stories add entries.
