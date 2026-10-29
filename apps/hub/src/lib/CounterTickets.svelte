<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { KitchenTicketView, SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { alerts, ackAlerts } from "./alerts.svelte";
  import { hasPermission, newIdempotencyKey, submitIntent } from "./auth";

  /**
   * The POS open-tickets strip (Story 7.4): a row listing each open walk-up kitchen ticket with its
   * number and stage ("K-2043 READY"). Tapping an entry opens its lines and notes and, for a
   * Ready ticket, acknowledges its order-ready alert. The next-stage button is `kitchen.advance`.
   */
  let {
    tickets,
    staff,
    onchanged = () => {},
  }: {
    tickets: KitchenTicketView[];
    staff: SessionStaff | null;
    /** A ticket moved: the page reloads what it shows. */
    onchanged?: () => void;
  } = $props();

  const STAGES: Record<string, MessageKey> = {
    new: "hub.order.ticket_stage.new",
    preparing: "hub.order.ticket_stage.preparing",
    ready: "hub.order.ticket_stage.ready",
  };
  const NEXT: Record<string, { to: string; label: MessageKey }> = {
    new: { to: "preparing", label: "hub.pos.tickets.start" },
    preparing: { to: "ready", label: "hub.order.mark_ready" },
    ready: { to: "served", label: "hub.order.mark_served" },
  };

  let openId: string | null = $state(null);
  let busy = $state(false);
  let error: string | null = $state(null);
  const open = $derived(tickets.find((tk) => tk.id === openId) ?? null);
  const canAdvance = $derived(hasPermission(staff, "kitchen.use"));

  function pick(tk: KitchenTicketView) {
    openId = openId === tk.id ? null : tk.id;
    error = null;
    const alert = alerts.open.find((a) => a.kind === "order_ready" && a.ticket_id === tk.id);
    if (alert && openId === tk.id) void ackAlerts([alert.id]);
  }

  async function advance(tk: KitchenTicketView) {
    const next = NEXT[tk.stage];
    if (busy || !next) return;
    busy = true;
    error = null;
    const out = await submitIntent(
      { kind: "kitchen.advance", ticket_id: tk.id, to: next.to },
      newIdempotencyKey(),
      tk.rev,
    ).finally(() => (busy = false));
    if (out.kind === "applied") return void onchanged();
    error = t("hub.order.ticket_stale");
    onchanged();
  }
</script>

{#if tickets.length}
  <section class="strip" aria-label={t("hub.pos.tickets.label")} data-testid="open-tickets">
    <ul class="entries">
      {#each tickets as tk (tk.id)}
        <li>
          <button
            type="button"
            class="ui-interactive entry type-hub-label"
            data-testid="open-ticket"
            data-ticket={tk.id}
            data-stage={tk.stage}
            aria-expanded={openId === tk.id}
            onclick={() => pick(tk)}
            >{t("hub.pos.tickets.entry", {
              number: tk.number,
              stage: t(STAGES[tk.stage] ?? "hub.order.ticket_stage.new").toUpperCase(),
            })}</button
          >
        </li>
      {/each}
    </ul>
    {#if open}
      <div class="detail" data-testid="open-ticket-detail">
        <ul class="type-hub-label">
          {#each open.lines as l (l.line_id)}
            <li>
              <span>{t("hub.bill.item", { qty: l.qty, name: l.label })}</span>
              {#each l.notes as note, i (i)}<span class="muted"> · {note}</span>{/each}
            </li>
          {/each}
        </ul>
        {#if canAdvance && NEXT[open.stage]}
          <Button
            label={NEXT[open.stage].label}
            variant="secondary"
            disabled={busy}
            disabledReason="hub.sheet.waiting"
            onclick={() => advance(open)}
          />
        {/if}
        {#if error}<p class="type-hub-label error" role="alert">{error}</p>{/if}
      </div>
    {/if}
  </section>
{/if}

<style>
  .strip {
    display: grid;
    gap: var(--space-2);
    padding-block: var(--space-2);
  }

  .entries {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .entry {
    min-height: 44px;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background: var(--surface);
    color: var(--text);
  }

  .entry[data-stage="ready"] {
    border-color: var(--state-order-pending);
    font-weight: 700;
  }

  .detail {
    display: grid;
    gap: var(--space-2);
  }

  .muted {
    color: var(--text-muted);
  }

  .error {
    color: var(--danger);
  }
</style>
