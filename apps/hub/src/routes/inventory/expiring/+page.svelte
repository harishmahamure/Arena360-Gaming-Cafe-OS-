<script lang="ts">
  import { Button, TextField, t } from "@arena/ui";
  import type { ExpiryView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    hasPermission,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { catalog } from "../../../lib/catalog.svelte";
  import { KEY_EXPIRY_WARN_DAYS, loadExpiry, parseWarnDays } from "../../../lib/expiry";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import { priceText } from "../../../lib/products";

  let view: ExpiryView | null = $state(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let warn = $state("");
  let warnRev = $state<number | null>(null);
  let error: MessageKey | undefined = $state();
  let saving = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let loads = 0;
  let idem: { value: string; key: string } | null = null;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
  const exponent = $derived(branchLocale.current?.exponent ?? 2);
  const symbol = $derived(branchLocale.current?.symbol ?? "");

  const money = (minor: number) => {
    const parts = [symbol, priceText(minor, exponent)];
    return (branchLocale.current?.symbol_position === "after" ? parts.reverse() : parts).join("");
  };

  async function load(keepWarn = false) {
    const mine = ++loads;
    const [list, settings] = await Promise.all([loadExpiry(), loadSettings()]);
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out" || settings.kind === "signed_out") {
      return void (await invalidate(SESSION_DEPENDENCY));
    }
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") {
      view = list.view;
      if (!keepWarn) warn = String(list.view.warn_days);
    }
    if (settings.kind === "ok") {
      warnRev = settings.settings.find((s) => s.key === KEY_EXPIRY_WARN_DAYS)?.rev ?? null;
    }
  }

  onMount(() => void load());
  $effect(() => {
    if (catalog.version) untrack(() => void load(true));
  });

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!canEdit || saving || warnRev === null) return;
    const days = parseWarnDays(warn);
    status = null;
    error = days === null ? "hub.expiry.warn_invalid" : undefined;
    if (days === null) return;
    saving = true;
    const value = String(days);
    if (idem?.value !== value) idem = { value, key: newIdempotencyKey() };
    const outcome = await submitIntent(
      { kind: "settings.set", key: KEY_EXPIRY_WARN_DAYS, value },
      idem.key,
      warnRev,
    ).finally(() => (saving = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      idem = null;
      status = { tone: "ok", message: "hub.settings.saved" };
    } else {
      const code = outcome.kind === "error" ? outcome.error.code : null;
      if (code === "VALIDATION_FAILED") error = "hub.expiry.warn_invalid";
      status = {
        tone: "refusal",
        message:
          code === "STALE"
            ? "hub.settings.branch.stale"
            : code === "PERMISSION_DENIED"
              ? "hub.settings.branch.manager_only"
              : "hub.settings.failed",
      };
    }
    await load(false);
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.expiry.title") })}</title>
</svelte:head>

<section class="stock" aria-labelledby="expiring-title">
  <h1 class="type-hub-numeric" id="expiring-title">{t("hub.nav.inventory")}</h1>
  <h2 class="type-hub-title">{t("hub.expiry.title")}</h2>
  <p class="intro type-hub-body">{t("hub.expiry.intro")}</p>
  <a class="link type-hub-label" href="/inventory">{t("hub.expiry.back")}</a>
  <a class="link type-hub-label" href="/inventory/expired">{t("hub.expiry.expired_link")}</a>

  {#if canEdit}
    <form class="form" onsubmit={save} data-testid="expiry-warn-form">
      <TextField label="hub.expiry.warn_days" bind:value={warn} {error} inputmode="numeric" />
      <p class="help type-hub-label">{t("hub.expiry.warn_help")}</p>
      <div class="actions">
        <Button label="hub.expiry.warn_save" type="submit" />
      </div>
    </form>
  {:else if view}
    <p class="type-hub-label" data-testid="expiry-warn-value">
      {t("hub.expiry.warn_days")}: {view.warn_days}
    </p>
  {/if}

  <div
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="expiry-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </div>

  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.expiry.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading || !view}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else if view.expiring.length === 0}
    <p class="type-hub-body">{t("hub.expiry.empty")}</p>
  {:else}
    <ul class="rows" data-testid="expiring-list">
      {#each view.expiring as lot (lot.lot_id)}
        <li class="row" data-testid="expiring-row" data-lot-id={lot.lot_id}>
          <span class="type-hub-title">{lot.product}</span>
          <span class="type-hub-label"
            >{t("hub.expiry.row", { qty: lot.qty, date: lot.expiry })}</span
          >
          <span class="type-hub-label">{t("hub.expiry.col_value")}: {money(lot.value_minor)}</span>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .stock,
  .form {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .intro,
  .help {
    color: var(--text-muted);
  }

  .link {
    color: var(--text);
  }

  .rows {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background-color: var(--surface);
  }

  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }
</style>
