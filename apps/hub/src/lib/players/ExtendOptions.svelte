<script lang="ts">
  import { Button, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { OwnedPlanView, SnapshotLocale } from "@arena/ui/generated";
  import { untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, loadWallet } from "../auth";
  import { timeText } from "../plans";
  import type { ExtendChoice } from "../tile-actions.svelte";
  import { walletEvents } from "../wallet.svelte";

  const MINUTES = [30, 60, 120] as const;
  const MINUTES_LABEL: Record<(typeof MINUTES)[number], MessageKey> = {
    30: "hub.extend.opt.30",
    60: "hub.extend.opt.60",
    120: "hub.extend.opt.120",
  };

  let {
    playerId,
    groupId,
    ownPackId,
    rateMinor,
    locale,
    disabled = false,
    onconfirm,
    oncancel,
  }: {
    playerId: string;
    groupId: string;
    ownPackId: string | null;
    rateMinor: number | null;
    locale: SnapshotLocale | null;
    disabled?: boolean;
    onconfirm: (choice: ExtendChoice) => void;
    oncancel: () => void;
  } = $props();

  let packs: OwnedPlanView[] = $state([]);
  let balance: number | null = $state(null);
  let headroom: number | null = $state(null);
  let loaded = $state(false);
  let minutes: (typeof MINUTES)[number] | null = $state(null);
  let source: { source: ExtendChoice["source"]; pack_id?: string } | null = $state(null);
  let turn = 0;

  async function refresh() {
    const mine = ++turn;
    const outcome = await loadWallet(playerId);
    if (mine !== turn) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    const owned = outcome.kind === "ok" ? (outcome.value.plans ?? []) : [];
    packs = owned.filter(
      (p) => p.plan_kind === "pack" && p.group_ids.includes(groupId) && p.id !== ownPackId,
    );
    balance = outcome.kind === "ok" ? outcome.value.balance_minor : null;
    const credit = outcome.kind === "ok" ? outcome.value.credit : null;
    headroom =
      credit && credit.limit_minor > 0 && !credit.paused
        ? credit.limit_minor - credit.outstanding_minor
        : null;
    loaded = true;
  }

  $effect(() => {
    void playerId;
    void groupId;
    void walletEvents.version;
    untrack(() => void refresh());
  });

  const price = (min: number) =>
    rateMinor === null ? null : Math.floor((rateMinor * min + 30) / 60);
  const money = (minor: number) => (locale ? formatMoney(minor, locale) : "");
  const usable = (p: OwnedPlanView) => !p.expired && (p.minutes_left ?? 0) > 0;
  const chosenPack = (p: OwnedPlanView) => source?.source === "pack" && source.pack_id === p.id;

  const ready = $derived(minutes !== null && source !== null);
  const needs = $derived(minutes === null ? "hub.extend.pick_time" : "hub.extend.pick_source");

  function confirm() {
    if (minutes === null || source === null) return;
    onconfirm({ minutes, ...source });
  }
</script>

<div class="extend" role="group" aria-labelledby="extend-title" data-testid="extend-options">
  <p class="type-hub-label" id="extend-title">{t("hub.extend.title")}</p>
  <div class="row" role="group" aria-label={t("hub.extend.time")}>
    {#each MINUTES as m (m)}
      {@const cost = price(m)}
      <button
        type="button"
        class="ui-interactive option type-hub-body tabular-nums"
        aria-pressed={minutes === m}
        data-testid="extend-minutes-{m}"
        onclick={() => !disabled && (minutes = m)}
      >
        {t(MINUTES_LABEL[m])}{cost === null ? "" : ` · ${money(cost)}`}
      </button>
    {/each}
  </div>
  <p class="type-hub-label" id="extend-source-title">{t("hub.extend.source")}</p>
  <div class="column" role="group" aria-labelledby="extend-source-title">
    {#if !loaded}
      <p class="type-hub-label" role="status">{t("hub.session.opt.loading")}</p>
    {/if}
    {#if balance !== null && rateMinor !== null}
      <button
        type="button"
        class="ui-interactive option type-hub-body tabular-nums"
        aria-pressed={source?.source === "wallet"}
        data-testid="extend-source-wallet"
        onclick={() => !disabled && (source = { source: "wallet" })}
      >
        {t("hub.extend.wallet", { balance: money(balance) })}
      </button>
    {/if}
    {#each packs as p (p.id)}
      <button
        type="button"
        class="ui-interactive option type-hub-body tabular-nums"
        class:expired={!usable(p)}
        aria-pressed={chosenPack(p)}
        aria-disabled={!usable(p)}
        data-testid="extend-source-pack"
        onclick={() => !disabled && usable(p) && (source = { source: "pack", pack_id: p.id })}
      >
        {#if usable(p)}
          {t("hub.extend.pack", { name: p.name, time: timeText(p.minutes_left ?? 0) })}
        {:else}
          <span aria-hidden="true">⚠</span>
          {t("hub.extend.pack_expired", { name: p.name })}
        {/if}
      </button>
    {/each}
    {#if headroom !== null && rateMinor !== null}
      <button
        type="button"
        class="ui-interactive option type-hub-body tabular-nums"
        aria-pressed={source?.source === "credit"}
        data-testid="extend-source-credit"
        onclick={() => !disabled && (source = { source: "credit" })}
      >
        {t("hub.extend.credit", { headroom: money(Math.max(headroom, 0)) })}
      </button>
    {/if}
  </div>
  <div class="row">
    <Button
      label="hub.extend.confirm"
      disabled={disabled || !ready}
      disabledReason={disabled ? "hub.sheet.waiting" : needs}
      onclick={confirm}
    />
    <Button label="hub.extend.cancel" variant="secondary" onclick={oncancel} />
  </div>
</div>

<style>
  .extend {
    display: grid;
    gap: var(--space-2);
    width: 100%;
  }

  p {
    margin: 0;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .column {
    display: grid;
    gap: var(--space-2);
  }

  .option {
    --ui-bg: var(--button-secondary-background);
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    color: var(--button-secondary-foreground);
    font: inherit;
    text-align: start;
    cursor: pointer;
  }

  .option[aria-pressed="true"] {
    border-width: 2px;
    border-color: var(--accent);
  }

  .expired {
    color: var(--text-muted);
    cursor: not-allowed;
  }
</style>
