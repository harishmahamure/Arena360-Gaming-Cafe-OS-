<script lang="ts">
  import { CodeField, InlinePin, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { DiscoveredHub } from "@arena/ui/generated";
  import { onDestroy, onMount, tick } from "svelte";
  import {
    DISCOVERY_FALLBACK_MS,
    onDiscoveryDone,
    onDiscoveryFound,
    pairInfo,
    pairKiosk,
    probeIp,
    selectHub,
    startDiscovery,
    type DiscoveryEvent,
    type PairInput,
    type PairResult,
  } from "./bridge";

  let { onpaired }: { onpaired?: (branch: string) => void } = $props();

  type Phase = "searching" | "done" | "pairing" | "paired";

  let phase = $state<Phase>("searching");
  let hubs: DiscoveredHub[] = $state([]);
  let ip = $state("");
  let checking = $state(false);
  let ipError: MessageKey | null = $state(null);
  let pickError: MessageKey | null = $state(null);
  let selected: DiscoveredHub | null = $state(null);
  let announcement = $state("");
  let ipInput: HTMLInputElement | undefined = $state();
  let pairingHeading: HTMLHeadingElement | undefined = $state();
  let searchHeading: HTMLHeadingElement | undefined = $state();
  let run = 0;
  let current: number | null = null;
  let early: Array<{ done: boolean; event: DiscoveryEvent }> = [];
  let fallback: ReturnType<typeof setTimeout> | undefined;

  let mode = $state<"code" | "pin">("code");
  let code = $state("");
  let pcName = $state("");
  let pairing = $state(false);
  let codeError: MessageKey | null = $state(null);
  let nameError: MessageKey | null = $state(null);
  let pinPausedFor = $state(0);
  let pausedTimer: ReturnType<typeof setInterval> | null = null;
  let pairedBranch = $state("");
  let codeField: { focus: () => void } | undefined = $state();
  let nameInput: HTMLInputElement | undefined = $state();
  let pinToggle: HTMLButtonElement | undefined = $state();
  let pairedHeading: HTMLHeadingElement | undefined = $state();
  const CONNECT_TIMEOUT_MS = 30_000;
  let connectFailed = $state(false);
  let connectTimer: ReturnType<typeof setTimeout> | undefined;
  const shortFingerprint = (fp: string) =>
    (fp.toLowerCase().match(/[0-9a-f]{4}/g) ?? []).slice(0, 4).join(" ");
  const nameHintId = "pairing-name-hint";
  const nameErrorId = "pairing-name-error";

  const compatible = $derived(hubs.filter((h) => h.compatible));
  const offerIp = $derived(phase === "done" && compatible.length === 0);
  const ipHintId = "discovery-ip-hint";
  const ipErrorId = "discovery-ip-error";
  const ipDescribedBy = $derived(ipError ? [ipErrorId, ipHintId].join(" ") : ipHintId);
  const noteId = (hub: DiscoveredHub) => "incompatible-" + hub.fingerprint;

  function announce(text: string) {
    if (announcement !== text) announcement = text;
  }

  async function search() {
    const mine = ++run;
    phase = "searching";
    hubs = [];
    ipError = null;
    pickError = null;
    selected = null;
    current = null;
    early = [];
    clearTimeout(fallback);
    announce(t("player.discovery.searching"));
    const started = await startDiscovery();
    if (mine !== run) return;
    if (started === null) {
      phase = "done";
      pickError = "player.discovery.unavailable";
      announce(t("player.discovery.unavailable"));
      return;
    }
    current = started;
    fallback = setTimeout(() => {
      if (mine === run && phase === "searching") void finish(hubs);
    }, DISCOVERY_FALLBACK_MS);
    const queued = early;
    early = [];
    for (const q of queued) {
      if (q.done) onDone(q.event);
      else onFound(q.event);
    }
  }

  function ours(event: DiscoveryEvent, done: boolean): boolean {
    if (current === null) {
      if (phase === "searching") early.push({ done, event });
      return false;
    }
    return event.search === current;
  }

  function onFound(event: DiscoveryEvent) {
    if (!ours(event, false) || phase !== "searching") return;
    hubs = event.hubs;
    announce(t("player.discovery.announce_found", { count: event.hubs.length }));
  }

  function onDone(event: DiscoveryEvent) {
    if (!ours(event, true) || phase !== "searching") return;
    void finish(event.hubs);
  }

  async function finish(found: DiscoveredHub[]) {
    clearTimeout(fallback);
    hubs = found;
    phase = "done";
    if (found.some((h) => h.compatible)) {
      announce(t("player.discovery.announce_found", { count: found.length }));
      return;
    }
    announce(t("player.discovery.none"));
    await tick();
    ipInput?.focus();
  }

  async function choose(hub: DiscoveredHub) {
    if (pairing) return;
    pickError = null;
    const result = await selectHub(hub);
    if (result === "selected") {
      selected = hub;
      phase = "pairing";
      mode = "code";
      code = "";
      codeError = null;
      nameError = null;
      announce(t("player.pairing.title"));
      await tick();
      pairingHeading?.focus();
      const info = await pairInfo();
      if (info.pcName && pcName === "") pcName = info.pcName;
      return;
    }
    const message: MessageKey =
      result === "no_hub" ? "player.discovery.no_hub" : "player.discovery.unavailable";
    pickError = message;
    announce(t(message));
  }

  async function check(e: SubmitEvent) {
    e.preventDefault();
    const value = ip.trim();
    if (!value || checking) return;
    checking = true;
    ipError = null;
    const result = await probeIp(value);
    checking = false;
    if (result.kind === "found") {
      await choose(result.hub);
      if (phase === "pairing") return;
      ipError = pickError;
      pickError = null;
    } else {
      ipError =
        result.kind === "incompatible"
          ? "player.discovery.hub_incompatible"
          : result.kind === "no_hub"
            ? "player.discovery.no_hub"
            : "player.discovery.unavailable";
      announce(t(ipError));
    }
    await tick();
    ipInput?.focus();
  }

  function stopPause() {
    if (pausedTimer !== null) clearInterval(pausedTimer);
    pausedTimer = null;
  }

  function pausePin(seconds: number) {
    stopPause();
    pinPausedFor = Math.max(1, Math.ceil(seconds));
    pausedTimer = setInterval(() => {
      pinPausedFor = Math.max(0, pinPausedFor - 1);
      if (pinPausedFor === 0) stopPause();
    }, 1000);
  }

  function validName(): string | null {
    const name = pcName.trim();
    const chars = [...name].length;
    if (chars === 0 || chars > 63 || /\p{Cc}/u.test(name)) {
      nameError = "player.pairing.pc_name_invalid";
      return null;
    }
    nameError = null;
    return name;
  }

  function waitForConnection() {
    clearTimeout(connectTimer);
    connectFailed = false;
    connectTimer = setTimeout(() => {
      connectFailed = true;
      announce(t("player.pairing.connect_failed"));
    }, CONNECT_TIMEOUT_MS);
  }

  async function retryConnection() {
    waitForConnection();
    await tick();
    pairedHeading?.focus();
  }

  async function pairAgain() {
    clearTimeout(connectTimer);
    connectFailed = false;
    await again();
  }

  function failureOf(kind: PairResult["kind"]): MessageKey {
    switch (kind) {
      case "code_invalid":
        return "player.pairing.code_invalid";
      case "no_hub":
        return "player.pairing.no_hub";
      case "invalid":
        return "player.pairing.refused_name";
      case "not_primary":
        return "player.pairing.not_primary";
      case "hub_slow":
        return "player.pairing.hub_slow";
      case "hub_error":
        return "player.pairing.hub_error";
      case "already_paired":
        return "player.pairing.already_paired";
      default:
        return "player.pairing.unavailable";
    }
  }

  async function paired(branch: string) {
    pairedBranch = branch;
    phase = "paired";
    waitForConnection();
    announce(t("player.pairing.paired", { branch }));
    onpaired?.(branch);
    await tick();
    pairedHeading?.focus();
  }

  async function submitCode(entered = code) {
    if (pairing) return;
    const name = validName();
    if (name === null) {
      await tick();
      nameInput?.focus();
      return;
    }
    if (entered.length !== 6) {
      codeError = "player.pairing.code_incomplete";
      codeField?.focus();
      return;
    }
    pairing = true;
    codeError = null;
    const result = await pairKiosk({ code: entered }, name);
    pairing = false;
    if (result.kind === "paired") return void (await paired(result.branchName));
    code = "";
    codeError = failureOf(result.kind);
    await tick();
    codeField?.focus();
  }

  async function submitPin(pin: string) {
    const name = validName();
    if (name === null) return { ok: false, message: "player.pairing.pc_name_invalid" } as const;
    const input: PairInput = { pin };
    pairing = true;
    const result = await pairKiosk(input, name);
    pairing = false;
    if (result.kind === "paired") {
      void paired(result.branchName);
      return { ok: true } as const;
    }
    if (result.kind === "pin_rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (result.kind === "pin_paused") {
      pausePin(result.retryAfterS);
      mode = "code";
      announce(t("player.pairing.pin_paused", { seconds: pinPausedFor }));
      await tick();
      codeField?.focus();
      return { ok: true } as const;
    }
    return { ok: false, message: failureOf(result.kind) } as const;
  }

  function usePin() {
    if (pinPausedFor > 0) return;
    mode = "pin";
    codeError = null;
  }

  async function useCode() {
    mode = "code";
    await tick();
    pinToggle?.focus();
  }

  function onPairSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (mode === "code") void submitCode();
  }

  onDestroy(() => {
    stopPause();
    clearTimeout(connectTimer);
  });

  async function again() {
    if (pairing) return;
    await search();
    await tick();
    searchHeading?.focus();
  }

  onMount(() => {
    const stops: Array<() => void> = [];
    let gone = false;
    void (async () => {
      const add = (stop: () => void) => (gone ? stop() : stops.push(stop));
      add(await onDiscoveryFound(onFound));
      add(await onDiscoveryDone(onDone));
      if (!gone) await search();
    })();
    return () => {
      gone = true;
      run++;
      clearTimeout(fallback);
      for (const stop of stops) stop();
    };
  });
</script>

<main class="discovery" data-testid="discovery">
  <p class="brand type-kiosk-eyebrow">{t("app.name")}</p>
  {#if phase === "paired"}
    <h1
      class="type-kiosk-headline"
      tabindex="-1"
      bind:this={pairedHeading}
      data-testid="paired-heading"
    >
      {t("player.pairing.paired", { branch: pairedBranch })}
    </h1>
    {#if connectFailed}
      <p class="error type-kiosk-body" data-testid="connect-failed">
        <span aria-hidden="true">⚠</span>
        <span>{t("player.pairing.connect_failed")}</span>
      </p>
      <div class="actions">
        <button
          type="button"
          class="ui-interactive action primary type-kiosk-label"
          onclick={retryConnection}
        >
          {t("player.pairing.retry")}
        </button>
        <button type="button" class="ui-interactive action type-kiosk-label" onclick={pairAgain}>
          {t("player.pairing.pair_again")}
        </button>
      </div>
    {:else}
      <p class="busy type-kiosk-body">
        <span class="spinner" aria-hidden="true"></span>
        <span>{t("player.pairing.connecting")}</span>
      </p>
    {/if}
  {:else if phase === "pairing" && selected}
    <h1 class="type-kiosk-headline" tabindex="-1" bind:this={pairingHeading}>
      {t(mode === "pin" ? "player.pairing.pin_title" : "player.pairing.title")}
    </h1>
    <p class="type-kiosk-body" data-testid="selected-hub">
      {t("player.pairing.selected", { branch: selected.branch_name, hub: selected.hub_name })}
    </p>
    <p class="type-kiosk-body" data-testid="selected-fingerprint">
      {t("player.pairing.fingerprint", { fingerprint: shortFingerprint(selected.fingerprint) })}
    </p>
    <form class="pair" onsubmit={onPairSubmit} data-testid="pair-form">
      <div class="name">
        <label class="type-kiosk-label" for="pairing-name">{t("player.pairing.pc_name")}</label>
        <input
          id="pairing-name"
          class="ui-interactive field type-kiosk-body"
          type="text"
          autocomplete="off"
          spellcheck="false"
          maxlength="63"
          bind:value={pcName}
          bind:this={nameInput}
          aria-invalid={nameError ? "true" : undefined}
          aria-describedby={nameError ? [nameErrorId, nameHintId].join(" ") : nameHintId}
        />
        <p class="hint type-kiosk-label" id={nameHintId}>{t("player.pairing.pc_name_hint")}</p>
        {#if nameError}
          <p class="error type-kiosk-body" id={nameErrorId} data-testid="name-error">
            <span aria-hidden="true">⚠</span>
            <span>{t(nameError)}</span>
          </p>
        {/if}
      </div>
      {#if mode === "code"}
        <div class="code" data-testid="pair-code">
          <CodeField
            bind:this={codeField}
            bind:value={code}
            length={6}
            label="player.pairing.code_label"
            error={codeError}
            busy={pairing}
            oncomplete={(entered) => void submitCode(entered)}
          />
        </div>
        <p class="hint type-kiosk-body">{t("player.pairing.code_hint")}</p>
        <div class="actions">
          <button
            type="submit"
            class="ui-interactive action primary type-kiosk-label"
            aria-disabled={pairing ? "true" : undefined}
          >
            {t(pairing ? "player.pairing.pairing" : "player.pairing.submit")}
          </button>
          <button
            type="button"
            class="ui-interactive action type-kiosk-label"
            bind:this={pinToggle}
            disabled={pinPausedFor > 0}
            aria-describedby={pinPausedFor > 0 ? "pairing-pin-paused" : undefined}
            onclick={usePin}
            data-testid="use-pin"
          >
            {t("player.pairing.use_pin")}
          </button>
        </div>
        {#if pinPausedFor > 0}
          <p class="hint type-kiosk-body" id="pairing-pin-paused" data-testid="pin-paused">
            {t("player.pairing.pin_paused", { seconds: pinPausedFor })}
          </p>
        {/if}
      {/if}
    </form>
    {#if mode === "pin"}
      <div class="pin" data-testid="pair-pin">
        <InlinePin
          message="player.pairing.pin_prompt"
          submitLabel="player.pairing.submit"
          onsubmit={submitPin}
          oncancel={useCode}
        />
      </div>
    {/if}
    <div class="actions">
      <button
        type="button"
        class="ui-interactive action type-kiosk-label"
        disabled={pairing}
        onclick={again}
        data-testid="change-hub"
      >
        {t("player.pairing.change_hub")}
      </button>
    </div>
  {:else}
    <h1 class="type-kiosk-headline" tabindex="-1" bind:this={searchHeading}>
      {t("player.discovery.title")}
    </h1>
    {#if phase === "searching"}
      <p class="busy type-kiosk-body" data-testid="discovery-busy">
        <span class="spinner" aria-hidden="true"></span>
        <span>{t("player.discovery.searching")}</span>
      </p>
    {/if}
    {#if hubs.length > 0}
      <ul class="hubs" aria-label={t("player.discovery.found_list")} data-testid="hub-list">
        {#each hubs as hub (hub.fingerprint)}
          <li>
            <button
              type="button"
              class="ui-interactive hub type-kiosk-label"
              disabled={!hub.compatible || pairing}
              aria-describedby={hub.compatible ? undefined : noteId(hub)}
              onclick={() => choose(hub)}
            >
              {t("player.discovery.hub_option", { branch: hub.branch_name, hub: hub.hub_name })}
            </button>
            {#if !hub.compatible}
              <span class="note type-kiosk-label" id={noteId(hub)}>
                {t("player.discovery.incompatible")}
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if pickError && !offerIp}
      <p class="error type-kiosk-body" data-testid="pick-error">{t(pickError)}</p>
    {/if}
    {#if phase === "done"}
      {#if offerIp}
        {#if hubs.length === 0}
          <p class="hint type-kiosk-body">{t("player.discovery.none")}</p>
        {/if}
        <form class="ip" onsubmit={check} data-testid="ip-form">
          <label class="type-kiosk-label" for="discovery-ip">{t("player.discovery.ip_label")}</label
          >
          <input
            id="discovery-ip"
            class="ui-interactive field type-kiosk-body"
            type="text"
            inputmode="decimal"
            autocomplete="off"
            spellcheck="false"
            bind:value={ip}
            bind:this={ipInput}
            aria-invalid={ipError ? "true" : undefined}
            aria-describedby={ipDescribedBy}
          />
          <p class="hint type-kiosk-label" id={ipHintId}>{t("player.discovery.ip_hint")}</p>
          {#if ipError}
            <p class="error type-kiosk-body" id={ipErrorId} data-testid="ip-error">
              <span aria-hidden="true">⚠</span>
              <span>{t(ipError)}</span>
            </p>
          {/if}
          <div class="actions">
            <button
              type="submit"
              class="ui-interactive action primary type-kiosk-label"
              aria-disabled={checking || ip.trim() === "" ? "true" : undefined}
            >
              {t(checking ? "player.discovery.checking" : "player.discovery.check")}
            </button>
            <button type="button" class="ui-interactive action type-kiosk-label" onclick={again}>
              {t("player.discovery.search_again")}
            </button>
          </div>
        </form>
      {:else}
        <div class="actions">
          <button type="button" class="ui-interactive action type-kiosk-label" onclick={again}>
            {t("player.discovery.search_again")}
          </button>
        </div>
      {/if}
    {/if}
  {/if}
  <p class="visually-hidden" role="status" aria-live="polite" data-testid="discovery-status">
    {announcement}
  </p>
</main>

<style>
  .discovery {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: var(--space-6);
    min-height: 100vh;
    padding: var(--space-kiosk-margin);
    color: var(--text);
    text-align: center;
  }

  h1 {
    max-width: 24ch;
    margin: 0 auto;
  }

  h1:focus {
    outline: none;
  }

  .brand,
  .hint,
  .note {
    margin: 0;
    color: var(--text-muted);
  }

  .busy {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    margin: 0;
    color: var(--text-muted);
  }

  .spinner {
    width: var(--space-5);
    height: var(--space-5);
    border: 3px solid var(--line);
    border-top-color: var(--accent);
    border-radius: var(--radius-full);
    animation: spin 1s linear infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation: none;
      border-color: var(--accent);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .hubs {
    display: grid;
    gap: var(--space-3);
    width: min(40rem, 100%);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .hubs li {
    display: grid;
    gap: var(--space-2);
  }

  .hub,
  .action {
    min-height: 48px;
    padding: var(--space-3) var(--space-5);
    border: 2px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface);
    color: var(--text);
    cursor: pointer;
  }

  .hub {
    width: 100%;
  }

  .hub:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .action.primary {
    border-color: var(--accent);
    background-color: var(--accent);
    color: var(--on-accent);
  }

  .action[aria-disabled="true"] {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .pair,
  .name {
    display: grid;
    gap: var(--space-3);
    width: min(32rem, 100%);
    text-align: left;
  }

  .code,
  .pin {
    display: grid;
    justify-items: center;
  }

  .ip {
    display: grid;
    gap: var(--space-3);
    width: min(32rem, 100%);
    text-align: left;
  }

  .field {
    min-height: 48px;
    padding: var(--space-2) var(--space-4);
    border: 2px solid var(--text-field-border);
    border-radius: var(--radius-md);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
  }

  .field[aria-invalid="true"] {
    border-color: var(--text-field-border-error);
  }

  .error {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    color: var(--text-field-error-text);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--space-4);
  }
</style>
