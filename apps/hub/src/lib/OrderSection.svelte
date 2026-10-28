<script lang="ts">
  import { Button, ReasonPicker, formatMoney, t } from "@arena/ui";
  import type { BillItem, ReasonCode, SnapshotLocale } from "@arena/ui/generated";
  import { tick } from "svelte";
  import { CANCEL_LABELS, CANCEL_REASONS, VOID_LABELS, VOID_REASONS } from "./tile-actions.svelte";

  /**
   * The products placed on a session bill (Story 5.13): each line with its state, Mark served for
   * a line that is not served yet, and Cancel (a reason, then "Was it already made?"). A served
   * line offers neither. With `onvoid` the whole order can be voided (Story 5.23): a reason (and
   * the text of Other), then the Hub asks for a manager's PIN when the signed-in member lacks it.
   */
  let {
    items,
    locale,
    busy,
    onserve,
    oncancel,
    onvoid,
  }: {
    items: BillItem[];
    locale: SnapshotLocale;
    busy: boolean;
    onserve: (lineId: string) => void;
    oncancel: (lineId: string, made: boolean, reason: ReasonCode) => void;
    /** Voids the whole order (a reason, and the text of `other`); absent when it cannot be voided. */
    onvoid?: (reason: ReasonCode, note?: string) => void;
  } = $props();

  const OPTIONS = CANCEL_REASONS.map((value) => ({ value, label: CANCEL_LABELS[value] }));
  const VOID_OPTIONS = VOID_REASONS.map((value) => ({ value, label: VOID_LABELS[value] }));
  const pending = $derived(items.filter((i) => !i.served).length);

  /** The line being cancelled, and the reason once it is chosen (then the made question). */
  let cancelling: string | null = $state(null);
  let reason: ReasonCode | null = $state(null);
  let root: HTMLElement | undefined = $state();
  let voiding = $state(false);

  $effect(() => {
    // An order that left the bill closes its flow.
    if (!onvoid) voiding = false;
  });

  async function stopVoid() {
    voiding = false;
    await tick();
    root?.querySelector<HTMLElement>('[data-testid="order-void"] button')?.focus(); // i18n-ignore (a selector)
  }

  function chooseVoid(value: string, note?: string) {
    voiding = false;
    onvoid?.(value as ReasonCode, note);
  }

  $effect(() => {
    // A line that left the bill or was served closes its flow.
    if (cancelling && !items.some((i) => i.line_id === cancelling && !i.served)) {
      cancelling = null;
      reason = null;
    }
  });

  async function stop() {
    const id = cancelling;
    cancelling = null;
    reason = null;
    await tick();
    root?.querySelector<HTMLElement>(`[data-line="${id}"] button`)?.focus(); // i18n-ignore (a selector)
  }

  function chooseReason(value: string) {
    reason = value as ReasonCode;
  }

  function answer(made: boolean) {
    const id = cancelling;
    const why = reason;
    if (!id || !why) return;
    cancelling = null;
    reason = null;
    oncancel(id, made, why);
  }
</script>

<section class="orders" data-testid="order-section" bind:this={root}>
  <h3 class="type-hub-label title">{t("hub.order.title")}</h3>
  <ul class="type-hub-label">
    {#each items as item (item.line_id)}
      <li data-testid="order-line" data-line={item.line_id} data-served={item.served}>
        <div class="row">
          <span class="name">{t("hub.bill.item", { qty: item.qty, name: item.label })}</span>
          <span class="state" data-testid="order-line-state">
            {#if item.served}<span aria-hidden="true">✓</span> {t("hub.order.served")}
            {:else}<span aria-hidden="true">•</span> {t("hub.order.unserved")}{/if}
          </span>
          <span class="amount tabular-nums">{formatMoney(item.amount_minor, locale)}</span>
        </div>
        {#if !item.served && cancelling !== item.line_id}
          <div class="actions">
            <Button
              label="hub.order.mark_served"
              disabled={busy}
              disabledReason="hub.sheet.waiting"
              onclick={() => onserve(item.line_id)}
            />
            <Button
              label="hub.order.cancel"
              variant="secondary"
              disabled={busy}
              disabledReason="hub.sheet.waiting"
              onclick={() => {
                cancelling = item.line_id;
                reason = null;
              }}
            />
          </div>
        {/if}
        {#if cancelling === item.line_id}
          {#if !reason}
            <div data-testid="order-cancel-reason">
              <ReasonPicker
                options={OPTIONS}
                legend="hub.order.cancel_legend"
                onconfirm={chooseReason}
                oncancel={() => void stop()}
              />
            </div>
          {:else}
            <div class="made" role="group" aria-labelledby="made-{item.line_id}">
              <p class="type-hub-label" id="made-{item.line_id}">{t("hub.order.made_legend")}</p>
              <p class="type-hub-label muted">{t("hub.order.made_note")}</p>
              <div class="actions">
                <Button
                  label="hub.order.made_yes"
                  variant="secondary"
                  disabled={busy}
                  disabledReason="hub.sheet.waiting"
                  onclick={() => answer(true)}
                />
                <Button
                  label="hub.order.made_no"
                  variant="secondary"
                  disabled={busy}
                  disabledReason="hub.sheet.waiting"
                  onclick={() => answer(false)}
                />
                <Button
                  label="hub.order.cancel_back"
                  variant="secondary"
                  onclick={() => void stop()}
                />
              </div>
            </div>
          {/if}
        {/if}
      </li>
    {/each}
  </ul>
  {#if pending > 0}
    <p class="type-hub-label muted" data-testid="order-pending">
      {t("hub.order.unserved_note")}
    </p>
  {/if}
  {#if onvoid}
    {#if voiding}
      <div data-testid="order-void-reason">
        <ReasonPicker
          options={VOID_OPTIONS}
          legend="hub.order.void_legend"
          noteFor="other"
          noteLabel="hub.order.void_note"
          onconfirm={chooseVoid}
          oncancel={() => void stopVoid()}
        />
      </div>
    {:else}
      <div class="actions" data-testid="order-void">
        <Button
          label="hub.order.void"
          variant="secondary"
          disabled={busy}
          disabledReason="hub.sheet.waiting"
          onclick={() => (voiding = true)}
        />
      </div>
    {/if}
  {/if}
</section>

<style>
  .orders {
    display: grid;
    gap: var(--space-2);
    width: 100%;
  }

  .title,
  p,
  ul {
    margin: 0;
  }

  ul {
    display: grid;
    gap: var(--space-3);
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    gap: var(--space-2);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    justify-content: space-between;
  }

  .name {
    flex: 1;
  }

  .state,
  .muted {
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .made {
    display: grid;
    gap: var(--space-2);
  }
</style>
