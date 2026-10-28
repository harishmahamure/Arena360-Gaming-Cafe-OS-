<script lang="ts">
  import { Button, InlinePin, TextField, formatDateTime, t } from "@arena/ui";
  import type { DeviceScanView, IntentPayload, LibraryEntryView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    elevate,
    hasPermission,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { library } from "../../../lib/catalog.svelte";
  import { branchLocale } from "../../../lib/locale.svelte";
  import {
    KINDS,
    KIND_LABELS,
    LAUNCHERS,
    LAUNCHER_LABELS,
    checkForm,
    loadLibrary,
    loadScans,
    parseTags,
    tagsText,
    type EntryForm,
  } from "../../../lib/library";
  import { sessionStaff } from "../../../lib/operating";

  type Job = { payload: IntentPayload; rev?: number; key: string; removing?: boolean };

  const INVALID: Record<string, MessageKey> = {
    entry_kind: "hub.games.invalid.kind",
    name: "hub.games.invalid.name",
    launcher: "hub.games.invalid.launcher",
    executable_path: "hub.games.invalid.executable_path",
    launch_uri: "hub.games.invalid.launch_uri",
    launch_args: "hub.games.invalid.launch_args",
    category: "hub.games.invalid.category",
    tags: "hub.games.invalid.tags",
    age_rating: "hub.games.invalid.age_rating",
    catalog_id: "hub.games.invalid.catalog_id",
  };
  const BLANK: EntryForm = {
    kind: "game",
    name: "",
    launcher: "steam",
    executablePath: "",
    launchUri: "",
    launchArgs: "",
    category: "",
    tags: "",
    ageRating: "",
    catalogId: "",
    enabled: true,
  };

  let entries: LibraryEntryView[] = $state([]);
  let loading = $state(true);
  let loadFailed = $state(false);
  let scans: DeviceScanView[] = $state([]);
  let scansFailed = $state(false);
  let editing: LibraryEntryView | null = $state(null);
  let form: EntryForm = $state({ ...BLANK });
  let errors: Record<string, MessageKey> = $state({});
  let busy = $state(false);
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let formEl: HTMLFormElement | undefined = $state();

  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "games.manage"));
  /** A linked entry's name, launcher and age rating are the catalogue's: shown, not edited. */
  const linked = $derived((editing as LibraryEntryView | null)?.catalog_id != null);

  let loads = 0;
  let idem: { json: string; key: string } | null = null;

  function keyFor(payload: IntentPayload) {
    const json = JSON.stringify(payload);
    if (idem?.json !== json) idem = { json, key: newIdempotencyKey() };
    return idem.key;
  }

  async function load() {
    const mine = ++loads;
    const list = await loadLibrary();
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") entries = list.entries;
  }

  let scanLoads = 0;
  async function loadScanList() {
    const mine = ++scanLoads;
    const list = await loadScans();
    if (mine !== scanLoads) return;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    scansFailed = list.kind !== "ok";
    if (list.kind === "ok") scans = list.devices;
  }

  onMount(() => {
    void load();
    void loadScanList();
  });
  // The Hub pushes `library.changed` when anyone writes the library or a PC reports a scan:
  // reload both lists.
  $effect(() => {
    if (library.version)
      untrack(() => {
        void load();
        void loadScanList();
      });
  });

  function reset(entry: LibraryEntryView | null = null) {
    editing = entry;
    errors = {};
    form = entry
      ? {
          kind: entry.kind,
          name: entry.name,
          launcher: entry.launcher,
          executablePath: entry.executable_path,
          launchUri: entry.launch_uri,
          launchArgs: entry.launch_args,
          category: entry.category,
          tags: tagsText(entry.tags),
          ageRating: entry.age_rating,
          catalogId: entry.catalog_id ?? "",
          enabled: entry.enabled,
        }
      : { ...BLANK };
    if (entry) void tick().then(() => formEl?.querySelector("input")?.focus());
  }

  async function run(job: Job, ticket?: string) {
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = job;
      return;
    }
    if (busy) return;
    busy = true;
    status = null;
    errors = {};
    const outcome = await submitIntent(job.payload, job.key, job.rev, {
      elevationTicket: ticket,
    }).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      idem = null;
      status = { tone: "ok", message: job.removing ? "hub.games.removed" : "hub.settings.saved" };
      reset();
      return void (await load());
    }
    const err = outcome.kind === "error" ? outcome.error : null;
    if (err && err.code !== "PIN_REQUIRED") idem = null;
    if (err?.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = job;
      return;
    }
    if (err?.code === "VALIDATION_FAILED" && err.field && INVALID[err.field]) {
      return void (errors = { [err.field]: INVALID[err.field] });
    }
    const stale =
      err?.code === "STALE" ||
      (err?.code === "VALIDATION_FAILED" && (err.field === "id" || err.field === "base_rev"));
    status = {
      tone: "refusal",
      message: stale
        ? "hub.games.stale"
        : err?.code === "PERMISSION_DENIED"
          ? "hub.refusal.permission"
          : "hub.settings.failed",
    };
    if (stale) {
      await load();
      if (editing) reset(entries.find((e) => e.id === editing?.id) ?? null);
    }
  }

  async function approve(pin: string) {
    const job = approving;
    if (!job) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate([{ intent_kind: job.payload.kind }], pin);
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    approving = null;
    void run(job, outcome.ticket);
    return { ok: true } as const;
  }

  function save(event: SubmitEvent) {
    event.preventDefault();
    const bad = checkForm(form, linked);
    if (Object.keys(bad).length) {
      errors = bad;
      return void tick().then(() =>
        formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus(),
      );
    }
    const catalogId = form.catalogId.trim();
    const payload: IntentPayload = {
      kind: "library.upsert",
      id: editing?.id,
      entry_kind: form.kind,
      name: form.name.trim(),
      launcher: form.launcher,
      executable_path: form.executablePath.trim(),
      launch_uri: form.launchUri.trim(),
      launch_args: form.launchArgs.trim(),
      category: form.category.trim(),
      tags: parseTags(form.tags),
      age_rating: form.ageRating.trim(),
      enabled: form.enabled,
      catalog_id: catalogId || undefined,
    };
    void run({ payload, rev: editing?.rev, key: keyFor(payload) });
  }

  function remove(entry: LibraryEntryView) {
    const payload: IntentPayload = { kind: "library.delete", id: entry.id };
    void run({ payload, rev: entry.rev, key: keyFor(payload), removing: true });
  }

  const launcherText = (launcher: string) =>
    t(LAUNCHER_LABELS[launcher as keyof typeof LAUNCHER_LABELS] ?? "hub.games.launcher.none");
  const scanTime = (at: number) => formatDateTime(at, branchLocale.current?.tz);
  const scanLaunchers = (scan: DeviceScanView) =>
    scan.launchers && scan.launchers.length > 0
      ? t("hub.games.scans.launchers", {
          names: scan.launchers.map((l) => launcherText(l.launcher)).join(", "),
        })
      : t("hub.games.scans.no_launchers");
  const kindText = (kind: string) =>
    t(KIND_LABELS[kind as keyof typeof KIND_LABELS] ?? "hub.games.kind.game");
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.settings.nav.games") })}</title>
</svelte:head>

<section class="games" aria-labelledby="games-title">
  <h2 class="type-hub-title" id="games-title">{t("hub.games.title")}</h2>
  <p class="intro type-hub-body">{t("hub.games.intro")}</p>

  <form class="form" bind:this={formEl} onsubmit={save} data-testid="game-form">
    <fieldset class="field type-hub-label">
      <legend>{t("hub.games.kind")}</legend>
      <div class="inline">
        {#each KINDS as k (k)}
          <label class="check type-hub-label"
            ><input type="radio" name="game-kind" value={k} bind:group={form.kind} />{t(
              KIND_LABELS[k],
            )}</label
          >
        {/each}
      </div>
      {#if errors.entry_kind}<p class="error" role="alert">{t(errors.entry_kind)}</p>{/if}
    </fieldset>

    {#if linked}
      <p class="hint type-hub-label" data-testid="game-linked">{t("hub.games.catalog_locked")}</p>
      <dl class="facts type-hub-label">
        <dt>{t("hub.games.name")}</dt>
        <dd data-testid="game-locked-name">{form.name}</dd>
        <dt>{t("hub.games.launcher")}</dt>
        <dd data-testid="game-locked-launcher">{launcherText(form.launcher)}</dd>
        <dt>{t("hub.games.age")}</dt>
        <dd data-testid="game-locked-age">{form.ageRating || t("hub.games.age.none")}</dd>
      </dl>
    {:else}
      <TextField label="hub.games.name" bind:value={form.name} error={errors.name} required />
      <label class="field type-hub-label">
        <span>{t("hub.games.launcher")}</span>
        <select
          class="ui-interactive select"
          bind:value={form.launcher}
          aria-invalid={errors.launcher ? "true" : undefined}
        >
          {#each LAUNCHERS as l (l)}
            <option value={l}>{t(LAUNCHER_LABELS[l])}</option>
          {/each}
        </select>
        {#if errors.launcher}<span class="error" role="alert">{t(errors.launcher)}</span>{/if}
      </label>
    {/if}
    <TextField
      label="hub.games.exe"
      bind:value={form.executablePath}
      error={errors.executable_path}
    />
    <TextField label="hub.games.uri" bind:value={form.launchUri} error={errors.launch_uri} />
    <TextField label="hub.games.args" bind:value={form.launchArgs} error={errors.launch_args} />
    <TextField label="hub.games.category" bind:value={form.category} error={errors.category} />
    <TextField label="hub.games.tags" bind:value={form.tags} error={errors.tags} />
    {#if !linked}
      <TextField label="hub.games.age" bind:value={form.ageRating} error={errors.age_rating} />
      {#if !editing}
        <TextField
          label="hub.games.catalog"
          bind:value={form.catalogId}
          error={errors.catalog_id}
        />
        <p class="hint type-hub-label">{t("hub.games.catalog_hint")}</p>
      {/if}
    {/if}
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.enabled} />{t("hub.games.enabled")}</label
    >
    <div class="actions">
      <Button label={editing ? "hub.games.save" : "hub.games.add"} type="submit" />
      {#if editing}<Button
          label="hub.games.cancel"
          variant="secondary"
          onclick={() => reset()}
        />{/if}
    </div>
  </form>

  <p
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="game-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </p>

  {#if approving}
    <div class="approval" data-testid="game-approval">
      <p class="type-hub-label">{t("hub.games.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <h2 class="type-hub-title">{t("hub.games.list")}</h2>
  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.games.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else if entries.length === 0}
    <p class="type-hub-body">{t("hub.games.empty")}</p>
  {:else}
    <ul class="rows" data-testid="game-list">
      {#each entries as e (e.id)}
        <li class="row" data-testid="game-row" data-entry-id={e.id}>
          <span class="type-hub-title" data-testid="game-name">{e.name}</span>
          <span class="type-hub-label" data-testid="game-kind">{kindText(e.kind)}</span>
          <span class="type-hub-label" data-testid="game-launcher">{launcherText(e.launcher)}</span>
          <span class="type-hub-label path" data-testid="game-path"
            >{e.executable_path || e.launch_uri}</span
          >
          {#if e.category}<span class="type-hub-label" data-testid="game-category"
              >{e.category}</span
            >{/if}
          {#if e.tags.length > 0}<span class="type-hub-label" data-testid="game-tags"
              >{tagsText(e.tags)}</span
            >{/if}
          {#if e.age_rating}<span class="type-hub-label" data-testid="game-age">{e.age_rating}</span
            >{/if}
          {#if e.catalog_id}<span class="badge type-hub-label">{t("hub.games.badge.catalog")}</span
            >{/if}
          {#if !e.enabled}<span class="badge type-hub-label">{t("hub.games.badge.disabled")}</span
            >{/if}
          <div class="actions">
            <button type="button" class="ui-interactive btn type-hub-label" onclick={() => reset(e)}
              >{t("hub.games.edit", { name: e.name })}</button
            >
            <button
              type="button"
              class="ui-interactive btn type-hub-label"
              onclick={() => remove(e)}>{t("hub.games.delete", { name: e.name })}</button
            >
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <h2 class="type-hub-title">{t("hub.games.scans.title")}</h2>
  <p class="intro type-hub-body">{t("hub.games.scans.intro")}</p>
  {#if scansFailed}
    <p class="type-hub-body" role="alert">{t("hub.games.scans.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void loadScanList()} />
  {:else if scans.length === 0}
    <p class="type-hub-body" data-testid="scan-empty">{t("hub.games.scans.empty")}</p>
  {:else}
    <ul class="rows" data-testid="scan-list">
      {#each scans as s (s.device_id)}
        <li class="row" data-testid="scan-row" data-device-id={s.device_id}>
          <span class="type-hub-title" data-testid="scan-name">{s.name}</span>
          {#if s.scanned_at === null}
            <span class="type-hub-label" data-testid="scan-last">{t("hub.games.scans.never")}</span>
          {:else}
            <span class="type-hub-label tabular-nums" data-testid="scan-last"
              >{t("hub.games.scans.last", { date: scanTime(s.scanned_at) })}</span
            >
            <span class="type-hub-label" data-testid="scan-count"
              >{t("hub.games.scans.found", { count: s.entry_count ?? 0 })}</span
            >
            <span class="type-hub-label" data-testid="scan-launchers">{scanLaunchers(s)}</span>
            {#if (s.skipped ?? 0) > 0}<span class="type-hub-label" data-testid="scan-skipped"
                >{t("hub.games.scans.skipped", { count: s.skipped ?? 0 })}</span
              >{/if}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .games,
  .form {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h2,
  p,
  dl,
  dd {
    margin: 0;
  }

  .intro,
  .hint {
    color: var(--text-muted);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
  }

  .inline {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-1) var(--space-4);
  }

  .facts dt {
    color: var(--text-muted);
  }

  .error {
    color: var(--text-field-error-text);
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
  }

  .select {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
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

  .path {
    overflow-wrap: anywhere;
    color: var(--text-muted);
  }

  .badge {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-inline-start: auto;
  }

  .form .actions {
    margin-inline-start: 0;
  }

  .btn {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    cursor: pointer;
  }

  .status {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
  }
</style>
