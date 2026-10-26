<script lang="ts">
  import { Button, InlinePin, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { IntentKind, PlayerProfile, SessionStaff } from "@arena/ui/generated";
  import { messages, type MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import {
    SESSION_DEPENDENCY,
    editPlayer,
    elevate,
    hasPermission,
    resetPlayerPassword,
    type IntentOutcome,
  } from "../auth";

  let {
    staff,
    profile,
    ondone,
    onstale,
    oncancel,
  }: {
    staff: SessionStaff | null;
    profile: PlayerProfile;
    ondone: (message: MessageKey) => void;
    onstale: () => void;
    oncancel: () => void;
  } = $props();

  const INVALID: Record<string, MessageKey> = {
    phone: "hub.players.invalid.phone",
    name: "hub.players.invalid.name",
    email: "hub.players.invalid.email",
    dob: "hub.players.invalid.dob",
    notes: "hub.players.invalid.notes",
    tags: "hub.players.invalid.tags",
    password: "hub.players.invalid.password",
    confirm: "hub.players.invalid.confirm",
  };

  const start = untrack(() => profile);
  let form = $state({
    phone: start.player.phone,
    name: start.player.name,
    email: start.email,
    dob: start.dob,
    notes: start.notes,
    tags: start.player.tags.join(", "),
    consent: start.consent?.granted ?? false,
  });
  let pw = $state({ password: "", confirm: "" });
  let errors: Record<string, MessageKey> = $state({});
  let busy = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let refusal: MessageKey | null = $state(null);
  let editEl: HTMLFormElement | undefined = $state();
  let resetEl: HTMLFormElement | undefined = $state();
  let active: HTMLFormElement | undefined;
  let pending: {
    kind: IntentKind;
    send: (ticket?: string) => Promise<IntentOutcome>;
    done: MessageKey;
  } | null = null;

  const needsApproval = $derived(!hasPermission(staff, "player.manage"));
  const tagList = () =>
    form.tags
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);

  function checkEdit() {
    const bad: Record<string, MessageKey> = {};
    if (!/^\+\d{8,15}$/.test(form.phone.replace(/[ -]/g, ""))) bad.phone = INVALID.phone;
    const name = [...form.name.trim()];
    if (name.length < 1 || name.length > 80) bad.name = INVALID.name;
    const email = form.email.trim();
    if (email && email.split("@").length !== 2) bad.email = INVALID.email;
    if (form.dob && form.dob >= new Date().toLocaleDateString("en-CA")) bad.dob = INVALID.dob;
    if ([...form.notes].length > 500) bad.notes = INVALID.notes;
    const tags = tagList();
    if (tags.length > 10 || tags.some((tag) => [...tag].length > 24)) bad.tags = INVALID.tags;
    return bad;
  }

  function checkPassword() {
    const bad: Record<string, MessageKey> = {};
    const n = [...pw.password].length;
    if (n < 6 || n > 128) bad.password = INVALID.password;
    else if (pw.password !== pw.confirm) bad.confirm = INVALID.confirm;
    return bad;
  }

  async function showErrors(next: Record<string, MessageKey>) {
    errors = next;
    await tick();
    focusFirstInvalid(active);
  }

  async function run(ticket?: string) {
    if (busy || !pending) return;
    busy = true;
    refusal = null;
    const outcome = await pending.send(ticket).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") return ondone(pending.done);
    const err = outcome.kind === "error" ? outcome.error : null;
    if (err?.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = true;
      return;
    }
    if (err?.code === "STALE") return onstale();
    if (err?.code === "PHONE_TAKEN") {
      return void (await showErrors({ phone: "player.signup.phone_taken" }));
    }
    if (err?.code === "VALIDATION_FAILED" && err.field && INVALID[err.field]) {
      return void (await showErrors({ [err.field]: INVALID[err.field] }));
    }
    refusal = err?.code === "PERMISSION_DENIED" ? "hub.refusal.permission" : "hub.settings.failed";
  }

  async function submit(
    event: SubmitEvent,
    el: HTMLFormElement | undefined,
    bad: () => Record<string, MessageKey>,
    next: NonNullable<typeof pending>,
  ) {
    event.preventDefault();
    if (busy) return;
    refusal = null;
    active = el;
    const found = bad();
    if (Object.keys(found).length) return void (await showErrors(found));
    errors = {};
    pending = next;
    if (needsApproval) {
      approvalPrompt = "ui.pin.prompt";
      approving = true;
      return;
    }
    await run();
  }

  const saveEdit = (event: SubmitEvent) =>
    submit(event, editEl, checkEdit, {
      kind: "player.update",
      done: "hub.players.edit.done",
      send: (ticket) =>
        editPlayer(
          {
            player_id: profile.player.player_id,
            base_rev: profile.rev,
            phone: form.phone,
            name: form.name,
            email: form.email,
            dob: form.dob,
            notes: form.notes,
            tags: tagList(),
            consent: form.consent,
            policy_version: messages["player.policy_version"],
          },
          ticket,
        ),
    });

  const savePassword = (event: SubmitEvent) =>
    submit(event, resetEl, checkPassword, {
      kind: "player.password_reset",
      done: "hub.players.reset.done",
      send: (ticket) =>
        resetPlayerPassword(
          { player_id: profile.player.player_id, password: pw.password, confirm: pw.confirm },
          ticket,
        ),
    });

  async function approve(pin: string) {
    if (!pending) return { ok: false, message: "ui.pin.unreachable" } as const;
    const outcome = await elevate([{ intent_kind: pending.kind }], pin);
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

  onMount(() => editEl?.querySelector<HTMLInputElement>('input[name="phone"]')?.focus());
</script>

<form class="form" bind:this={editEl} onsubmit={saveEdit} data-testid="player-edit-form">
  <h2 class="type-hub-title">{t("hub.players.edit.title")}</h2>
  <TextField
    label="hub.players.phone"
    name="phone"
    type="tel"
    bind:value={form.phone}
    error={errors.phone}
    autocomplete="off"
  />
  <TextField label="hub.players.name" name="name" bind:value={form.name} error={errors.name} />
  <TextField
    label="hub.players.email"
    name="email"
    type="email"
    bind:value={form.email}
    error={errors.email}
    autocomplete="off"
  />
  <div class="field type-hub-label">
    <label for="player-edit-dob">{t("hub.players.dob")}</label>
    <input
      id="player-edit-dob"
      class="ui-interactive control"
      type="date"
      bind:value={form.dob}
      aria-invalid={errors.dob ? "true" : undefined}
      aria-describedby={errors.dob ? "player-edit-dob-error" : undefined}
    />
    {#if errors.dob}<p class="error" id="player-edit-dob-error" role="alert">
        {t(errors.dob)}
      </p>{/if}
  </div>
  <div class="field type-hub-label">
    <label for="player-edit-notes">{t("hub.players.notes")}</label>
    <textarea
      id="player-edit-notes"
      class="ui-interactive control"
      rows="3"
      bind:value={form.notes}
      aria-invalid={errors.notes ? "true" : undefined}
      aria-describedby={errors.notes ? "player-edit-notes-error" : undefined}></textarea>
    {#if errors.notes}<p class="error" id="player-edit-notes-error" role="alert">
        {t(errors.notes)}
      </p>{/if}
  </div>
  <TextField label="hub.players.tags" name="tags" bind:value={form.tags} error={errors.tags} />
  <label class="check type-hub-label"
    ><input type="checkbox" name="consent" bind:checked={form.consent} />{t(
      "player.signup.consent",
    )}</label
  >
  <div class="actions">
    <Button label="hub.players.edit.save" type="submit" />
    <Button label="hub.players.cancel" variant="secondary" onclick={oncancel} />
  </div>
</form>

<form class="form" bind:this={resetEl} onsubmit={savePassword} data-testid="player-reset-form">
  <h2 class="type-hub-title">{t("hub.players.reset")}</h2>
  <TextField
    label="hub.players.reset.new"
    name="new-password"
    type="password"
    bind:value={pw.password}
    error={errors.password}
    autocomplete="new-password"
  />
  <TextField
    label="hub.players.reset.confirm"
    name="confirm-password"
    type="password"
    bind:value={pw.confirm}
    error={errors.confirm}
    autocomplete="new-password"
  />
  <div class="actions">
    <Button label="hub.players.reset.save" type="submit" />
  </div>
</form>

{#if refusal}<p class="error type-hub-label" role="alert">
    <span aria-hidden="true">⚠</span>
    {t(refusal)}
  </p>{/if}

{#if approving}
  <div class="approval" data-testid="player-approval">
    <p class="type-hub-label">{t("hub.players.approves")}</p>
    <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = false)} />
  </div>
{/if}

<style>
  .form {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h2,
  p {
    margin: 0;
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .error {
    color: var(--text-field-error-text);
  }

  .control {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
    font: inherit;
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }

  .check input {
    inline-size: var(--space-touch-min);
    block-size: var(--space-touch-min);
    flex: none;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
  }
</style>
