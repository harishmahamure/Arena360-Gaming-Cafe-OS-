<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { PlayerExtendOptions, SnapshotLocale } from "@arena/ui/generated";
  import { onMount, tick } from "svelte";
  import { playerExtend, playerExtendOptions } from "./bridge";
  import { EXTEND_STEPS, sourcesOf, type ExtendSourceChoice } from "./extend";
  import { refusalText, sourceText, stepText } from "./extend-text";

  // `extend-options` (Story 6.21): inside the expanded HUD panel, the three steps (+30 min, +1 h,
  // +2 h) with the prices the Hub quoted at the live rate, and the ways the player can pay. The
  // Hub applies the extend by the counter's own rules or refuses it with a typed reason; a
  // refusal leaves the panel open with the other options usable. There is no manager override
  // here. With the Hub unreachable every option is disabled.

  let {
    locale,
    connected,
    onadded,
  }: {
    /** The branch's money format, once the attract view has told it. */
    locale: SnapshotLocale | null;
    /** The Hub link is up (`false`: every option is disabled and the panel says why). */
    connected: boolean;
    /** The Hub added the time: the HUD says "More time added. GG." and closes the panel. */
    onadded: () => void;
  } = $props();

  type Phase = "loading" | "ready" | "unreachable" | "unavailable";
  let options = $state<PlayerExtendOptions | null>(null);
  let phase = $state<Phase>("loading");
  let minutes: number | null = $state(null);
  let sourceKey: string | null = $state(null);
  let busy = $state(false);
  /** The Hub did not answer the last extend. */
  let lost = $state(false);
  let refusal = $state<{ code: string; choice: ExtendSourceChoice } | null>(null);
  let root: HTMLDivElement | undefined = $state();

  const sources = $derived(options ? sourcesOf(options) : []);
  const offline = $derived(!connected || phase === "unreachable" || lost);
  const steps = $derived(
    options ? options.quotes : EXTEND_STEPS.map((m) => ({ minutes: m, price_minor: null })),
  );
  const chosen = $derived(sources.find((s) => s.key === sourceKey) ?? null);
  const ready = $derived(
    phase === "ready" && !offline && !busy && minutes !== null && chosen !== null,
  );
  const line = $derived.by(() => {
    if (offline) return t("player.hub.reconnecting_retry");
    if (phase === "unavailable") return t("player.extend.unavailable");
    if (refusal && options && locale) {
      return refusalText(refusal.code, refusal.choice, options, locale);
    }
    return "";
  });

  const price = (minor: number | null): string =>
    minor !== null && locale ? formatMoney(minor, locale) : "";

  /** Reads the options; the first read also moves focus onto the first option. */
  async function load(first: boolean) {
    if (!connected) {
      phase = "unreachable";
      return;
    }
    const result = await playerExtendOptions();
    if (result.kind !== "ok") {
      phase = result.kind;
      return;
    }
    options = result.options;
    phase = "ready";
    lost = false;
    const keys = sourcesOf(result.options).map((s) => s.key);
    if (sourceKey === null || !keys.includes(sourceKey)) sourceKey = keys[0] ?? null;
    if (!first) return;
    await tick();
    root?.querySelector<HTMLInputElement>("input[name=extend-step]:not([disabled])")?.focus();
  }

  onMount(() => void load(true));

  // The link coming back after it dropped: ask again.
  $effect(() => {
    if (connected && phase === "unreachable") void load(true);
  });

  async function confirm() {
    if (!ready || minutes === null || chosen === null) return;
    busy = true;
    refusal = null;
    const outcome = await playerExtend({
      minutes,
      source: chosen.source,
      ...(chosen.packId === undefined ? {} : { pack_id: chosen.packId }),
    });
    busy = false;
    if (outcome.kind === "ok") return onadded();
    if (outcome.kind === "unreachable") {
      lost = true;
      return;
    }
    refusal = { code: outcome.code, choice: chosen };
    // The balance or a pack's hours may have moved: the line and the list say what is true now.
    await load(false);
  }
</script>

<div class="extend" bind:this={root} data-testid="extend-options">
  <p class="title type-kiosk-label">{t("player.extend.title")}</p>
  {#if phase === "loading" && connected}
    <p class="type-kiosk-body" role="status" data-testid="extend-loading">
      {t("player.extend.loading")}
    </p>
  {:else}
    <fieldset disabled={offline || busy || phase === "unavailable"}>
      <legend class="type-kiosk-body">{t("player.extend.how_much")}</legend>
      {#each steps as step (step.minutes)}
        <label class="choice type-kiosk-body" data-testid="extend-step">
          <input type="radio" name="extend-step" value={step.minutes} bind:group={minutes} />
          <span>{stepText(step.minutes)}</span>
          <span class="amount tabular-nums" data-testid="extend-price">
            {price(step.price_minor)}
          </span>
        </label>
      {/each}
    </fieldset>
    {#if options && phase === "ready"}
      <fieldset disabled={offline || busy}>
        <legend class="type-kiosk-body">{t("player.extend.pay_with")}</legend>
        {#each sources as source (source.key)}
          <label class="choice type-kiosk-body" data-testid="extend-source">
            <input type="radio" name="extend-source" value={source.key} bind:group={sourceKey} />
            <span>{locale ? sourceText(source, options, locale) : ""}</span>
          </label>
        {/each}
      </fieldset>
    {/if}
    <p class="line type-kiosk-body" role={line ? "alert" : undefined} data-testid="extend-line">
      {#if line}⚠ {line}{/if}
    </p>
    <button
      type="button"
      class="confirm type-kiosk-label"
      aria-disabled={ready ? undefined : "true"}
      data-testid="extend-confirm"
      onclick={() => void confirm()}
    >
      {busy ? t("player.extend.adding") : t("player.extend.confirm")}
    </button>
  {/if}
</div>

<style>
  .extend {
    display: grid;
    gap: var(--space-3);
  }

  .title,
  .line {
    margin: 0;
  }

  .title {
    color: var(--hud-toast-title);
  }

  fieldset {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }

  legend {
    padding: 0;
    margin-bottom: var(--space-1);
  }

  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 44px;
    padding: 0 var(--space-3);
    border: 2px solid var(--hud-toast-title);
    border-radius: var(--radius-xl);
    cursor: pointer;
  }

  .choice:has(input:checked) {
    background-color: var(--hud-toast-title);
    color: var(--hud-toast-background);
  }

  .choice:has(input:focus-visible) {
    outline: 3px solid var(--hud-toast-title);
    outline-offset: 2px;
  }

  .choice input {
    accent-color: var(--hud-toast-background);
  }

  .amount {
    margin-left: auto;
  }

  fieldset:disabled .choice {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .confirm {
    min-height: 44px;
    padding: var(--space-2) var(--space-5);
    border: 2px solid var(--hud-toast-title);
    border-radius: var(--radius-full);
    background-color: var(--button-warning-background);
    color: var(--button-warning-foreground);
    cursor: pointer;
  }

  .confirm[aria-disabled="true"] {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .confirm:focus-visible {
    outline: 3px solid var(--hud-toast-title);
    outline-offset: 2px;
  }

  /* HC: the shared 4px focus ring and 2px white outline, never the toast's own colours. */
  :global([data-theme="hc"]) .confirm,
  :global([data-theme="hc"]) .choice {
    outline: 2px solid var(--text);
    outline-offset: 0;
  }

  :global([data-theme="hc"]) .confirm:focus-visible,
  :global([data-theme="hc"]) .choice:has(input:focus-visible) {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }
</style>
