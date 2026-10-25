<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { KioskPairingCode } from "@arena/ui/generated";
  import { invalidate } from "$app/navigation";
  import { onDestroy, onMount, tick } from "svelte";
  import { SESSION_DEPENDENCY } from "./auth";
  import {
    countdown,
    discardPairingCode,
    groupCode,
    onPairingCode,
    requestPairingCode,
    type PairingChange,
  } from "./pairing";
  import { shortFingerprint } from "./snapshot";

  let { onclose, fingerprint = null }: { onclose: () => void; fingerprint?: string | null } =
    $props();

  let current = $state<KioskPairingCode | null>(null);
  let failed = $state(false);
  let replaced = $state(false);
  let now = $state(Date.now());
  let announcement = $state("");
  let heading: HTMLHeadingElement | undefined = $state();
  let announcedAt = new Set<number>();
  let fetching = false;
  let failures = 0;
  let retryAt = 0;
  let closed = false;

  const left = $derived(current ? current.expires_at - now : 0);

  async function fetchCode(renewal = false) {
    if (fetching) return;
    fetching = true;
    failed = false;
    replaced = false;
    const outcome = await requestPairingCode();
    fetching = false;
    if (closed) {
      if (outcome.kind === "ok") discardPairingCode();
      return;
    }
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind !== "ok") {
      current = null;
      failed = true;
      failures++;
      retryAt = Date.now() + Math.min(60_000, 5_000 * 2 ** (failures - 1));
      return;
    }
    failures = 0;
    retryAt = 0;
    show(outcome.code, renewal);
  }

  function show(code: KioskPairingCode, renewed: boolean) {
    current = code;
    failed = false;
    replaced = false;
    announcedAt = new Set();
    now = Date.now();
    if (renewed) {
      announcement = t("hub.settings.devices_display.pair_renewed", {
        code: groupCode(code.code),
      });
    }
  }

  function onChange(change: PairingChange) {
    if (closed) return;
    if (change === "replaced") {
      current = null;
      failed = false;
      replaced = true;
      announcement = t("hub.settings.devices_display.pair_replaced");
      return;
    }
    show(change, true);
  }

  function onTick() {
    now = Date.now();
    if (replaced) return;
    if (failed) {
      if (retryAt > 0 && now >= retryAt) void fetchCode(true);
      return;
    }
    if (!current) return;
    const minutes = Math.ceil(left / 60_000);
    for (const mark of [5, 1]) {
      if (minutes === mark && !announcedAt.has(mark) && left > 0) {
        announcedAt.add(mark);
        announcement =
          mark === 1
            ? t("hub.settings.devices_display.pair_one_minute_left")
            : t("hub.settings.devices_display.pair_minutes_left", { minutes: mark });
      }
    }
    if (left <= -2000) void fetchCode(true);
  }

  function close() {
    closed = true;
    discardPairingCode();
    onclose();
  }

  let stopListening: () => void = () => {};
  let timer: ReturnType<typeof setInterval> | undefined;

  onMount(() => {
    stopListening = onPairingCode(onChange);
    timer = setInterval(onTick, 1000);
    void fetchCode();
    void tick().then(() => heading?.focus());
  });

  onDestroy(() => {
    stopListening();
    clearInterval(timer);
    if (!closed) {
      closed = true;
      discardPairingCode();
    }
  });
</script>

<section class="panel" aria-labelledby="pair-kiosk-title" data-testid="pair-panel">
  <h3 class="type-hub-title" id="pair-kiosk-title" tabindex="-1" bind:this={heading}>
    {t("hub.settings.devices_display.pair_title")}
  </h3>
  <p class="note type-hub-body">{t("hub.settings.devices_display.pair_body")}</p>
  {#if fingerprint}
    <p class="note type-hub-label" data-testid="pair-fingerprint">
      {t("hub.settings.devices_display.pair_fingerprint", {
        fingerprint: shortFingerprint(fingerprint),
      })}
    </p>
  {/if}
  {#if replaced}
    <p class="note type-hub-label" data-testid="pair-replaced">
      {t("hub.settings.devices_display.pair_replaced")}
    </p>
    <Button
      label="hub.settings.devices_display.pair_take_back"
      variant="primary"
      onclick={() => void fetchCode()}
    />
  {:else if current}
    <p class="label type-hub-label" id="pair-code-label">
      {t("hub.settings.devices_display.pair_code")}
    </p>
    <p class="code" aria-labelledby="pair-code-label" data-testid="pair-code">
      {groupCode(current.code)}
    </p>
    <p class="expires type-hub-label" data-testid="pair-expires">
      {t("hub.settings.devices_display.pair_expires", { countdown: countdown(left) })}
    </p>
  {:else if failed}
    <p class="status refusal type-hub-label" data-testid="pair-failed">
      <span aria-hidden="true">⚠</span>
      <span>{t("hub.settings.devices_display.pair_failed")}</span>
    </p>
    <Button
      label="hub.settings.devices_display.pair_retry"
      variant="secondary"
      onclick={() => void fetchCode()}
    />
  {:else}
    <p class="note type-hub-label">{t("hub.settings.devices_display.pair_loading")}</p>
  {/if}
  <div class="actions">
    <Button label="hub.settings.devices_display.pair_close" variant="secondary" onclick={close} />
  </div>
  <p class="visually-hidden" role="status" aria-live="polite" data-testid="pair-announce">
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
  .expires {
    color: var(--text);
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
