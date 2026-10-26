<script lang="ts">
  import { Button, InlinePin, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { PlayerForm, SessionStaff } from "@arena/ui/generated";
  import { messages, type MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, createPlayer, elevate, hasPermission } from "../auth";

  let {
    staff,
    phone = "+91",
    oncreated,
    oncancel,
  }: {
    staff: SessionStaff | null;
    phone?: string;
    oncreated: (username: string) => void;
    oncancel: () => void;
  } = $props();

  const INVALID: Record<string, MessageKey> = {
    username: "hub.players.invalid.username",
    password: "hub.players.invalid.password",
    phone: "hub.players.invalid.phone",
    name: "hub.players.invalid.name",
    email: "hub.players.invalid.email",
    dob: "hub.players.invalid.dob",
    notes: "hub.players.invalid.notes",
    tags: "hub.players.invalid.tags",
  };

  let form = $state({
    username: "",
    password: "",
    phone: untrack(() => phone),
    name: "",
    email: "",
    dob: "",
    notes: "",
    tags: "",
    consent: false,
  });
  let errors: Record<string, MessageKey> = $state({});
  let busy = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let refusal: MessageKey | null = $state(null);
  let formEl: HTMLFormElement | undefined = $state();

  const needsApproval = $derived(!hasPermission(staff, "player.manage"));
  const tagList = () =>
    form.tags
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);

  function check() {
    const bad: Record<string, MessageKey> = {};
    if (!/^[a-z0-9._-]{3,32}$/.test(form.username.trim().toLowerCase())) {
      bad.username = INVALID.username;
    }
    const pw = [...form.password].length;
    if (pw < 6 || pw > 128) bad.password = INVALID.password;
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

  const build = (): PlayerForm => ({
    username: form.username.trim(),
    password: form.password,
    phone: form.phone,
    name: form.name,
    email: form.email,
    dob: form.dob,
    notes: form.notes,
    tags: tagList(),
    consent: form.consent,
    policy_version: messages["player.policy_version"],
  });

  async function showErrors(next: Record<string, MessageKey>) {
    errors = next;
    await tick();
    focusFirstInvalid(formEl);
  }

  async function run(ticket?: string) {
    if (busy) return;
    busy = true;
    refusal = null;
    const outcome = await createPlayer(build(), ticket).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") return oncreated(form.username.trim().toLowerCase());
    const err = outcome.kind === "error" ? outcome.error : null;
    if (err?.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = true;
      return;
    }
    if (err?.code === "USERNAME_TAKEN") {
      return void (await showErrors({ username: "player.signup.username_taken" }));
    }
    if (err?.code === "PHONE_TAKEN") {
      return void (await showErrors({ phone: "player.signup.phone_taken" }));
    }
    if (err?.code === "VALIDATION_FAILED" && err.field && INVALID[err.field]) {
      return void (await showErrors({ [err.field]: INVALID[err.field] }));
    }
    refusal = err?.code === "PERMISSION_DENIED" ? "hub.refusal.permission" : "hub.settings.failed";
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    refusal = null;
    const bad = check();
    if (Object.keys(bad).length) return void (await showErrors(bad));
    errors = {};
    if (needsApproval) {
      approvalPrompt = "ui.pin.prompt";
      approving = true;
      return;
    }
    await run();
  }

  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: "player.create" }], pin);
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

  onMount(() => formEl?.querySelector<HTMLInputElement>('input[name="username"]')?.focus());
</script>

<form class="form" bind:this={formEl} onsubmit={submit} data-testid="player-form">
  <h2 class="type-hub-title">{t("hub.players.title")}</h2>
  <TextField
    label="hub.players.username"
    name="username"
    bind:value={form.username}
    error={errors.username}
    autocomplete="off"
  />
  <TextField
    label="hub.players.password"
    name="password"
    type="password"
    bind:value={form.password}
    error={errors.password}
    autocomplete="new-password"
  />
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
    <label for="player-dob">{t("hub.players.dob")}</label>
    <input
      id="player-dob"
      class="ui-interactive control"
      type="date"
      bind:value={form.dob}
      aria-invalid={errors.dob ? "true" : undefined}
      aria-describedby={errors.dob ? "player-dob-error" : undefined}
    />
    {#if errors.dob}<p class="error" id="player-dob-error" role="alert">{t(errors.dob)}</p>{/if}
  </div>
  <div class="field type-hub-label">
    <label for="player-notes">{t("hub.players.notes")}</label>
    <textarea
      id="player-notes"
      class="ui-interactive control"
      rows="3"
      bind:value={form.notes}
      aria-invalid={errors.notes ? "true" : undefined}
      aria-describedby={errors.notes ? "player-notes-error" : undefined}></textarea>
    {#if errors.notes}<p class="error" id="player-notes-error" role="alert">
        {t(errors.notes)}
      </p>{/if}
  </div>
  <TextField label="hub.players.tags" name="tags" bind:value={form.tags} error={errors.tags} />
  <label class="check type-hub-label"
    ><input type="checkbox" bind:checked={form.consent} />{t("player.signup.consent")}</label
  >
  <div class="actions">
    <Button label="hub.players.submit" type="submit" />
    <Button label="hub.players.cancel" variant="secondary" onclick={oncancel} />
  </div>
  {#if refusal}<p class="error type-hub-label" role="alert">
      <span aria-hidden="true">⚠</span>
      {t(refusal)}
    </p>{/if}
</form>

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
