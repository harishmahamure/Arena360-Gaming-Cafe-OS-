<script lang="ts">
  import { Button, InlinePin, TextField, formatMoney, t } from "@arena/ui";
  import type { PlayerProfile, SessionStaff, WalletDisposal } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, elevate, erasePlayer, hasPermission } from "../auth";
  import { branchLocale } from "../locale.svelte";

  let {
    staff,
    profile,
    ondone,
    oncancel,
  }: {
    staff: SessionStaff | null;
    profile: PlayerProfile;
    ondone: () => void;
    oncancel: () => void;
  } = $props();

  const DISPOSALS: WalletDisposal[] = ["refunded", "forfeited"];
  const DISPOSAL_LABELS: Record<WalletDisposal, MessageKey> = {
    refunded: "hub.players.erase.wallet.refunded",
    forfeited: "hub.players.erase.wallet.forfeited",
  };

  const balance = $derived(profile.wallet_minor ?? 0);
  const needsApproval = $derived(!hasPermission(staff, "player.manage"));

  let wallet: WalletDisposal | null = $state(null);
  let note = $state("");
  let errors: { wallet?: MessageKey; note?: MessageKey } = $state({});
  let refusal: MessageKey | null = $state(null);
  let busy = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let root: HTMLElement | undefined = $state();

  function check() {
    const bad: typeof errors = {};
    if (balance > 0) {
      if (!wallet) bad.wallet = "hub.players.erase.invalid.wallet";
      const n = [...note.trim()].length;
      if (n < 1 || n > 120) bad.note = "hub.players.erase.invalid.note";
    }
    return bad;
  }

  async function run(ticket?: string) {
    if (busy) return;
    busy = true;
    refusal = null;
    const outcome = await erasePlayer(
      {
        player_id: profile.player.player_id,
        ...(balance > 0 && wallet ? { wallet, note: note.trim() } : {}),
      },
      ticket,
    ).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") return ondone();
    const err = outcome.kind === "error" ? outcome.error : null;
    if (err?.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = true;
      return;
    }
    if (err?.code === "ERASE_BLOCKED") {
      refusal = err.message.includes("session")
        ? "hub.players.erase.blocked.session"
        : "hub.players.erase.blocked.credit";
      return;
    }
    if (err?.code === "VALIDATION_FAILED" && err.field === "wallet") {
      errors = { wallet: "hub.players.erase.invalid.wallet" };
      return;
    }
    if (err?.code === "VALIDATION_FAILED" && err.field === "note") {
      errors = { note: "hub.players.erase.invalid.note" };
      return;
    }
    refusal = err?.code === "PERMISSION_DENIED" ? "hub.refusal.permission" : "hub.settings.failed";
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    refusal = null;
    errors = check();
    if (Object.keys(errors).length) return;
    if (needsApproval) {
      approvalPrompt = "ui.pin.prompt";
      approving = true;
      return;
    }
    await run();
  }

  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: "player.erased" }], pin);
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    approving = false;
    void run(outcome.ticket);
    return { ok: true } as const;
  }

  onMount(() => root?.querySelector<HTMLElement>("input,button")?.focus());
</script>

<section
  class="erase"
  bind:this={root}
  aria-labelledby="player-erase-title"
  data-testid="player-erase"
>
  <h2 class="type-hub-title" id="player-erase-title">{t("hub.players.erase.title")}</h2>
  <p class="type-hub-body">{t("hub.players.erase.intro")}</p>
  <div class="lists type-hub-body">
    <div data-testid="erase-erased">
      <h3 class="type-hub-label">{t("hub.players.erase.erased.title")}</h3>
      <ul>
        <li>{t("hub.players.erase.erased.name")}</li>
        <li>{t("hub.players.erase.erased.contact")}</li>
        <li>{t("hub.players.erase.erased.dob")}</li>
        <li>{t("hub.players.erase.erased.login")}</li>
      </ul>
    </div>
    <div data-testid="erase-kept">
      <h3 class="type-hub-label">{t("hub.players.erase.kept.title")}</h3>
      <ul>
        <li>{t("hub.players.erase.kept.money")}</li>
        <li>{t("hub.players.erase.kept.sessions")}</li>
        <li>{t("hub.players.erase.kept.consent")}</li>
      </ul>
    </div>
  </div>

  <form class="form" onsubmit={submit} data-testid="player-erase-form">
    {#if balance > 0 && branchLocale.current}
      <fieldset class="group" data-testid="erase-wallet">
        <legend class="type-hub-label">
          {t("hub.players.erase.wallet.title", {
            amount: formatMoney(balance, branchLocale.current),
          })}
        </legend>
        {#each DISPOSALS as value (value)}
          <label class="option type-hub-body">
            <input type="radio" name="erase-wallet" {value} bind:group={wallet} />
            <span>{t(DISPOSAL_LABELS[value])}</span>
          </label>
        {/each}
        {#if errors.wallet}<p class="error type-hub-label" role="alert">
            {t(errors.wallet)}
          </p>{/if}
      </fieldset>
      <TextField
        label="hub.players.erase.note"
        name="erase-note"
        bind:value={note}
        error={errors.note}
        autocomplete="off"
      />
    {/if}
    <div class="actions">
      <Button label="hub.players.erase.confirm" type="submit" />
      <Button label="hub.players.cancel" variant="secondary" onclick={oncancel} />
    </div>
  </form>

  {#if refusal}<p class="error type-hub-label" role="alert" data-testid="erase-refusal">
      <span aria-hidden="true">⚠</span>
      {t(refusal)}
    </p>{/if}

  {#if approving}
    <div class="approval" data-testid="player-approval">
      <p class="type-hub-label">{t("hub.players.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = false)} />
    </div>
  {/if}
</section>

<style>
  .erase,
  .form,
  .group,
  .approval {
    display: grid;
    gap: var(--space-3);
  }

  .erase {
    max-width: 48rem;
  }

  h2,
  h3,
  p,
  ul {
    margin: 0;
  }

  .lists {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-6);
  }

  ul {
    padding-inline-start: var(--space-4);
  }

  .group {
    margin: 0;
    padding: 0;
    border: 0;
  }

  .option {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }

  .option input {
    inline-size: var(--space-touch-min);
    block-size: var(--space-touch-min);
    flex: none;
  }

  .error {
    color: var(--text-field-error-text);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
