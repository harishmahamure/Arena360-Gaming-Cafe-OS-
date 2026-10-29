<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { KdsPairingCode } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { onDestroy, onMount, tick } from "svelte";
  import { SESSION_DEPENDENCY } from "./auth";
  import { discardKdsPairingCode, requestKdsPairingCode } from "./kds-pairing";
  import { countdown, groupCode } from "./pairing";

  // Pair a kitchen display (Story 7.6): the Hub shows where the tablet goes and a 6-digit
  // single-use code with a 10-minute countdown. Unlike a kiosk code it is not renewed: an
  // expired or used code is replaced by pressing "Get a new code". Closing discards it.

  let { onclose }: { onclose: () => void } = $props();

  let current = $state<KdsPairingCode | null>(null);
  let failure = $state<MessageKey | null>(null);
  let now = $state(Date.now());
  let announcement = $state("");
  let heading: HTMLHeadingElement | undefined = $state();
  let announcedAt = new Set<number>();
  let fetching = false;
  let closed = false;

  const left = $derived(current ? current.expires_at - now : 0);
  const expired = $derived(current !== null && left <= 0);

  async function fetchCode() {
    if (fetching) return;
    fetching = true;
    failure = null;
    const outcome = await requestKdsPairingCode();
    fetching = false;
    if (closed) {
      if (outcome.kind === "ok") discardKdsPairingCode();
      return;
    }
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind !== "ok") {
      current = null;
      failure =
        outcome.kind === "denied"
          ? "hub.settings.kitchen.pair_only_managers"
          : outcome.kind === "locked"
            ? "hub.settings.kitchen.locked"
            : "hub.settings.kitchen.pair_failed";
      return;
    }
    current = outcome.code;
    announcedAt = new Set();
    now = Date.now();
  }

  function onTick() {
    now = Date.now();
    if (!current || left <= 0) return;
    const minutes = Math.ceil(left / 60_000);
    for (const mark of [5, 1]) {
      if (minutes === mark && !announcedAt.has(mark)) {
        announcedAt.add(mark);
        announcement =
          mark === 1
            ? t("hub.settings.kitchen.pair_one_minute_left")
            : t("hub.settings.kitchen.pair_minutes_left", { minutes: mark });
      }
    }
  }

  function close() {
    closed = true;
    discardKdsPairingCode();
    onclose();
  }

  let timer: ReturnType<typeof setInterval> | undefined;

  onMount(() => {
    timer = setInterval(onTick, 1000);
    void fetchCode();
    void tick().then(() => heading?.focus());
  });

  onDestroy(() => {
    clearInterval(timer);
    if (!closed) {
      closed = true;
      discardKdsPairingCode();
    }
  });
</script>

<section class="panel" aria-labelledby="pair-kitchen-title" data-testid="pair-kitchen-panel">
  <h3 class="type-hub-title" id="pair-kitchen-title" tabindex="-1" bind:this={heading}>
    {t("hub.settings.kitchen.pair_title")}
  </h3>
  <p class="note type-hub-body">{t("hub.settings.kitchen.pair_body")}</p>
  {#if current}
    <p class="label type-hub-label" id="kitchen-address-label">
      {t("hub.settings.kitchen.pair_address")}
    </p>
    <p
      class="address type-hub-title"
      aria-labelledby="kitchen-address-label"
      data-testid="pair-kitchen-address"
    >
      {current.address ?? t("hub.settings.kitchen.pair_no_address")}
    </p>
    <p class="label type-hub-label" id="kitchen-code-label">
      {t("hub.settings.kitchen.pair_code")}
    </p>
    {#if expired}
      <p class="status refusal type-hub-label" data-testid="pair-kitchen-expired">
        <span aria-hidden="true">⚠</span>
        <span>{t("hub.settings.kitchen.pair_expired")}</span>
      </p>
      <Button
        label="hub.settings.kitchen.pair_new"
        variant="primary"
        onclick={() => void fetchCode()}
      />
    {:else}
      <p class="code" aria-labelledby="kitchen-code-label" data-testid="pair-kitchen-code">
        {groupCode(current.code)}
      </p>
      <p class="expires type-hub-label" data-testid="pair-kitchen-expires">
        {t("hub.settings.kitchen.pair_expires", { countdown: countdown(left) })}
      </p>
    {/if}
  {:else if failure}
    <p class="status refusal type-hub-label" data-testid="pair-kitchen-failed">
      <span aria-hidden="true">⚠</span>
      <span>{t(failure)}</span>
    </p>
    <Button
      label="hub.settings.devices_display.pair_retry"
      variant="secondary"
      onclick={() => void fetchCode()}
    />
  {:else}
    <p class="note type-hub-label">{t("hub.settings.kitchen.pair_loading")}</p>
  {/if}
  <div class="actions">
    <Button label="hub.settings.kitchen.pair_close" variant="secondary" onclick={close} />
  </div>
  <p class="visually-hidden" role="status" aria-live="polite" data-testid="pair-kitchen-announce">
    {announcement}
  </p>
</section>

<style>
  .panel {
    justify-self: stretch;
    display: grid;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface);
  }

  h3,
  p {
    margin: 0;
  }

  h3:focus {
    outline: none;
  }

  .note,
  .label,
  .expires,
  .address {
    color: var(--text);
  }

  .address {
    overflow-wrap: anywhere;
  }

  .code {
    color: var(--text);
    font-family: var(--type-web-display-family);
    font-size: var(--type-web-display-size);
    font-weight: var(--type-hub-numeric-weight);
    line-height: var(--type-web-display-line-height);
    font-variant-numeric: tabular-nums;
    white-space: pre;
  }

  .status {
    display: flex;
    gap: var(--space-2);
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }

  .actions {
    display: flex;
    gap: var(--space-3);
  }
</style>
