<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { KioskHubState, PlayerContext } from "@arena/ui/generated";
  import { onMount, tick } from "svelte";
  import { playerSignup } from "./bridge";
  import Legal from "./Legal.svelte";
  import {
    DEFAULT_PHONE,
    INVALID,
    consentParts,
    fieldProblem,
    firstInvalid,
    formProblems,
    isSignupField,
    type LegalPage,
    type SignupField,
    type SignupOutcome,
  } from "./signup";

  /**
   * The sign-up form (Story 6.10), in the login panel: username, password, phone, name and one
   * consent box. The fields go to the Kiosk service and nowhere else; the password is cleared
   * from the field the moment it is sent. The Hub creates the player and logs them in, so a
   * correct answer goes straight to the plan picker.
   */
  let {
    hub,
    onplayer,
    onback,
  }: {
    /** The Hub link as the service reports it (`null` until it says). */
    hub: KioskHubState | null;
    /** The player was created and is logged in. */
    onplayer: (player: PlayerContext) => void;
    /** Back to the log in form. */
    onback: () => void;
  } = $props();

  const id = $props.id();
  const errId = (field: SignupField): string => id + "-" + field + "-error";

  let username = $state("");
  let password = $state("");
  let phone = $state(DEFAULT_PHONE);
  let name = $state("");
  let consent = $state(false);
  let shown = $state(false);
  /** A request is out. */
  let sending = $state(false);
  /** The message under each field (a catalogue line), shown on blur, on submit and on a refusal. */
  let errors: Record<SignupField, string> = $state({
    username: "",
    password: "",
    phone: "",
    name: "",
  });
  /** The line under the form: the counter being unreachable, or a refusal with no field. */
  let problem = $state("");
  /** The legal page open over the form (the form stays mounted, so its fields are kept). */
  let legal: LegalPage | null = $state(null);

  const els: Record<SignupField, HTMLInputElement | undefined> = $state({
    username: undefined,
    password: undefined,
    phone: undefined,
    name: undefined,
  });
  const links: Record<LegalPage, HTMLButtonElement | undefined> = $state({
    terms: undefined,
    privacy: undefined,
  });

  const connected = $derived(hub === "connected");
  const parts = $derived(
    consentParts(t("player.signup.consent"), t("player.signup.terms"), t("player.signup.privacy")),
  );

  let gone = false;

  onMount(() => {
    els.username?.focus();
    return () => (gone = true);
  });

  const value = (field: SignupField): string =>
    field === "username"
      ? username
      : field === "password"
        ? password
        : field === "phone"
          ? phone
          : name;

  function blur(field: SignupField) {
    const key = fieldProblem(field, value(field));
    errors[field] = key ? t(key) : "";
  }

  /** Editing a field takes its message away (it is checked again on blur and on submit). */
  function edited(field: SignupField) {
    errors[field] = "";
    problem = "";
  }

  async function focusField(field: SignupField) {
    await tick();
    els[field]?.focus();
  }

  async function openLegal(page: LegalPage) {
    legal = page;
    await tick();
  }

  async function closeLegal() {
    const page = legal;
    legal = null;
    await tick();
    if (page) links[page]?.focus();
  }

  /** What the Hub's answer means for the form. */
  async function show(outcome: SignupOutcome) {
    if (outcome.kind === "ok") {
      onplayer(outcome.player);
      return;
    }
    if (outcome.kind === "unreachable") {
      problem = t("player.hub.reconnecting_retry");
      return;
    }
    const field = outcome.field;
    if (outcome.code === "USERNAME_TAKEN") {
      errors.username = t("player.signup.username_taken");
      return focusField("username");
    }
    if (outcome.code === "PHONE_TAKEN") {
      errors.phone = t("player.signup.phone_taken");
      return focusField("phone");
    }
    if (outcome.code === "VALIDATION_FAILED" && isSignupField(field)) {
      errors[field] = t(INVALID[field]);
      return focusField(field);
    }
    if (outcome.code === "VALIDATION_FAILED" && field === "consent") {
      problem = t("player.signup.consent_needed");
      return;
    }
    problem = t("player.signup.refused.other");
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (sending || !connected || !consent) return;
    const problems = formProblems({ username, password, phone, name });
    const bad = firstInvalid(problems);
    for (const f of ["username", "password", "phone", "name"] as const) {
      const key = problems[f];
      errors[f] = key ? t(key) : "";
    }
    if (bad) return focusField(bad);
    const fields = { username: username.trim(), password, phone: phone.trim(), name: name.trim() };
    // Sent, then gone: the field never keeps it, shown or not.
    password = "";
    shown = false;
    problem = "";
    sending = true;
    const outcome = await playerSignup({ ...fields, consent: true });
    sending = false;
    if (gone) return;
    await show(outcome);
  }
</script>

<div class="signup" data-testid="signup" data-legal={legal ?? undefined}>
  <form class="form" data-testid="signup-form" novalidate hidden={legal !== null} onsubmit={submit}>
    <h2 class="type-kiosk-title" id="login-title">{t("player.signup.title")}</h2>

    <div class="fields">
      <div class="field">
        <label class="type-kiosk-label" for="{id}-username">{t("player.login.username")}</label>
        <input
          bind:this={els.username}
          bind:value={username}
          class="input type-kiosk-body"
          class:invalid={errors.username}
          id="{id}-username"
          data-testid="signup-username"
          type="text"
          name="username"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          maxlength={64}
          aria-invalid={errors.username ? "true" : undefined}
          aria-describedby={errors.username ? errId("username") : undefined}
          oninput={() => edited("username")}
          onblur={() => blur("username")}
        />
        <p
          class="field-error tall type-kiosk-label"
          id={errId("username")}
          data-testid="signup-username-error"
          role={errors.username ? "alert" : undefined}
        >
          {#if errors.username}<span aria-hidden="true">⚠</span> {errors.username}{/if}
        </p>
      </div>

      <div class="field">
        <label class="type-kiosk-label" for="{id}-password">{t("player.login.password")}</label>
        <div class="row">
          <input
            bind:this={els.password}
            bind:value={password}
            class="input type-kiosk-body"
            class:invalid={errors.password}
            id="{id}-password"
            data-testid="signup-password"
            type={shown ? "text" : "password"}
            name="password"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            maxlength={256}
            aria-invalid={errors.password ? "true" : undefined}
            aria-describedby={errors.password ? errId("password") : undefined}
            oninput={() => edited("password")}
            onblur={() => blur("password")}
          />
          <button
            type="button"
            class="toggle type-kiosk-label"
            data-testid="signup-show"
            aria-controls="{id}-password"
            aria-pressed={shown ? "true" : "false"}
            onclick={() => (shown = !shown)}
          >
            {t(shown ? "ui.password.hide" : "ui.password.show")}
          </button>
        </div>
        <p
          class="field-error type-kiosk-label"
          id={errId("password")}
          data-testid="signup-password-error"
          role={errors.password ? "alert" : undefined}
        >
          {#if errors.password}<span aria-hidden="true">⚠</span> {errors.password}{/if}
        </p>
      </div>

      <div class="field">
        <label class="type-kiosk-label" for="{id}-phone">{t("player.signup.phone")}</label>
        <input
          bind:this={els.phone}
          bind:value={phone}
          class="input type-kiosk-body"
          class:invalid={errors.phone}
          id="{id}-phone"
          data-testid="signup-phone"
          type="tel"
          name="phone"
          autocomplete="off"
          maxlength={32}
          aria-invalid={errors.phone ? "true" : undefined}
          aria-describedby={errors.phone ? errId("phone") : undefined}
          oninput={() => edited("phone")}
          onblur={() => blur("phone")}
        />
        <p
          class="field-error type-kiosk-label"
          id={errId("phone")}
          data-testid="signup-phone-error"
          role={errors.phone ? "alert" : undefined}
        >
          {#if errors.phone}<span aria-hidden="true">⚠</span> {errors.phone}{/if}
        </p>
      </div>

      <div class="field">
        <label class="type-kiosk-label" for="{id}-name">{t("player.signup.name")}</label>
        <input
          bind:this={els.name}
          bind:value={name}
          class="input type-kiosk-body"
          class:invalid={errors.name}
          id="{id}-name"
          data-testid="signup-name"
          type="text"
          name="name"
          autocomplete="off"
          maxlength={120}
          aria-invalid={errors.name ? "true" : undefined}
          aria-describedby={errors.name ? errId("name") : undefined}
          oninput={() => edited("name")}
          onblur={() => blur("name")}
        />
        <p
          class="field-error type-kiosk-label"
          id={errId("name")}
          data-testid="signup-name-error"
          role={errors.name ? "alert" : undefined}
        >
          {#if errors.name}<span aria-hidden="true">⚠</span> {errors.name}{/if}
        </p>
      </div>
    </div>

    <div class="consent type-kiosk-body">
      <input
        class="check"
        type="checkbox"
        id="{id}-consent"
        data-testid="signup-consent"
        bind:checked={consent}
        onchange={() => (problem = "")}
      />
      <label for="{id}-consent" data-testid="signup-consent-text">
        {#each parts as part, n (n)}
          {#if part.page}
            <button
              type="button"
              class="link"
              data-testid="signup-link-{part.page}"
              bind:this={links[part.page]}
              onclick={() => part.page && openLegal(part.page)}>{part.text}</button
            >
          {:else}{part.text}{/if}
        {/each}
      </label>
    </div>

    <p
      class="error type-kiosk-label"
      data-testid="signup-error"
      role={problem ? "alert" : undefined}
    >
      {#if problem}<span aria-hidden="true">⚠</span> {problem}{/if}
    </p>

    <div class="actions">
      {#if !connected}
        <Button
          type="submit"
          label="player.signup.submit"
          disabled={true}
          disabledReason="player.hub.reconnecting_retry"
        />
      {:else if sending}
        <Button
          type="submit"
          label="player.signup.submit"
          disabled={true}
          disabledReason="player.signup.creating"
        />
      {:else if !consent}
        <Button
          type="submit"
          label="player.signup.submit"
          disabled={true}
          disabledReason="player.signup.consent_needed"
        />
      {:else}
        <Button type="submit" label="player.signup.submit" />
      {/if}
      <Button variant="secondary" label="player.signup.back" onclick={onback} />
    </div>
  </form>

  {#if legal}
    <Legal page={legal} onback={closeLegal} />
  {/if}
</div>

<style>
  .signup {
    display: grid;
    justify-items: stretch;
  }

  .form {
    display: grid;
    gap: var(--space-4);
    justify-items: stretch;
    width: min(44rem, 80vw);
    text-align: start;
  }

  .form[hidden] {
    display: none;
  }

  .form > * {
    margin: 0;
  }

  h2 {
    text-align: center;
  }

  /* Two columns where they fit: the form stays short enough for a 1366x768 screen. */
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 17rem), 1fr));
    gap: var(--space-1) var(--space-4);
  }

  .field {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
  }

  .field > * {
    margin: 0;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .input {
    flex: 1 1 12em;
    min-width: 0;
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
  }

  .input:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .input.invalid {
    border-width: 2px;
    border-color: var(--danger);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .toggle {
    min-width: var(--space-touch-min);
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--button-secondary-background);
    color: var(--button-secondary-foreground);
    cursor: pointer;
  }

  .toggle:focus-visible,
  .link:focus-visible,
  .check:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .field-error,
  .error {
    min-height: 1.5em;
    color: var(--text-field-error-text);
    font-weight: 600;
  }

  /* Room for the message is always kept, so showing one on blur never moves the next control
     out from under a click. */
  .field-error {
    min-height: 1.5em;
  }

  /* The username rule is long enough to wrap to a second line. */
  .field-error.tall {
    min-height: 3em;
  }

  .consent {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
  }

  /* The box itself is the 44px hit area. */
  .check {
    flex: none;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .consent label {
    flex: 1;
    min-width: 0;
    cursor: pointer;
  }

  .link {
    min-height: var(--space-touch-min);
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    align-items: flex-start;
  }
</style>
