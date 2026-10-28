<script lang="ts">
  // Settings → Games → "Found on PCs" (Story 6.5): the games and programs the PCs' scans found
  // that are not in the library and not dismissed. A Manager approves or dismisses several at
  // once; a Counter's change asks a manager's PIN in place (one PIN covers up to ten approvals).
  import { Button, InlinePin, t } from "@arena/ui";
  import type { IntentPayload, ProposalView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy, onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, elevate, newIdempotencyKey, submitIntent } from "./auth";
  import { library } from "./catalog.svelte";
  import {
    DISMISS_CHUNK,
    KIND_LABELS,
    LAUNCHER_LABELS,
    TICKET_USES,
    approvePayload,
    chunks,
    loadCover,
    loadProposals,
  } from "./library";

  type Job = { payload: IntentPayload; key: string };
  type Pending = { op: "approve" | "dismiss" | "clear"; jobs: Job[]; rest: Job[]; done: number };

  /** A change here needs a manager's PIN (the signed-in staff member lacks `games.manage`). */
  let { needsApproval }: { needsApproval: boolean } = $props();

  let proposals: ProposalView[] = $state([]);
  let dismissed = $state(0);
  let loading = $state(true);
  let loadFailed = $state(false);
  let selected: string[] = $state([]);
  let covers: Record<string, string> = $state({});
  let busy = $state(false);
  let pending: Pending | null = $state(null);
  let prompt: MessageKey = $state("ui.pin.prompt");
  let note: { tone: "ok" | "refusal"; text: string } | null = $state(null);

  let loads = 0;

  async function load() {
    const mine = ++loads;
    const list = await loadProposals();
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = list.kind !== "ok";
    if (list.kind !== "ok") return;
    proposals = list.proposals;
    dismissed = list.dismissed;
    selected = selected.filter((k) => list.proposals.some((p) => p.key === k));
    for (const p of list.proposals) {
      const sha = p.cover_sha256;
      if (sha && !(sha in covers)) {
        covers[sha] = "";
        void loadCover(sha).then((url) => {
          if (url) covers[sha] = url;
        });
      }
    }
  }

  onMount(() => void load());
  // The Hub pushes `library.changed` when a PC reports a scan or anyone writes the library.
  $effect(() => {
    if (library.version) untrack(() => void load());
  });
  onDestroy(() => {
    for (const url of Object.values(covers)) if (url) URL.revokeObjectURL(url);
  });

  const launcherText = (launcher: string) =>
    t(LAUNCHER_LABELS[launcher as keyof typeof LAUNCHER_LABELS] ?? "hub.games.launcher.none");
  const kindText = () => t(KIND_LABELS.game);
  const chosen = $derived(proposals.filter((p) => selected.includes(p.key)));
  const allChosen = $derived(proposals.length > 0 && chosen.length === proposals.length);

  function toggle(key: string, on: boolean) {
    selected = on ? [...selected, key] : selected.filter((k) => k !== key);
  }

  function toggleAll(on: boolean) {
    selected = on ? proposals.map((p) => p.key) : [];
  }

  const job = (payload: IntentPayload): Job => ({ payload, key: newIdempotencyKey() });

  /** Starts a change: straight away for a manager, after a PIN for a Counter. */
  async function start(op: Pending["op"], jobs: Job[]) {
    if (busy || jobs.length === 0) return;
    note = null;
    if (!needsApproval) return void (await execute({ op, jobs, rest: [], done: 0 }, undefined));
    // One PIN approves at most ten intents: the rest ask again.
    const [first, ...others] = chunks(jobs, TICKET_USES);
    prompt = "ui.pin.prompt";
    pending = { op, jobs: first, rest: others.flat(), done: 0 };
  }

  function approveSelected() {
    void start(
      "approve",
      chosen.map((p) => job(approvePayload(p))),
    );
  }

  function dismissSelected() {
    const keys = chosen.map((p) => p.key);
    void start(
      "dismiss",
      chunks(keys, DISMISS_CHUNK).map((k) => job({ kind: "library.dismiss", keys: k })),
    );
  }

  function clearDismissed() {
    void start("clear", [job({ kind: "library.dismiss_clear" })]);
  }

  async function execute(p: Pending, ticket: string | undefined) {
    busy = true;
    let done = p.done;
    let failed = 0;
    let signedOut = false;
    let pinAgain = false;
    for (const j of p.jobs) {
      const outcome = await submitIntent(j.payload, j.key, undefined, { elevationTicket: ticket });
      if (outcome.kind === "signed_out") {
        signedOut = true;
        break;
      }
      if (outcome.kind === "applied") done += 1;
      else if (outcome.kind === "error" && outcome.error.code === "PIN_REQUIRED") pinAgain = true;
      else failed += 1;
    }
    busy = false;
    if (signedOut) return void (await invalidate(SESSION_DEPENDENCY));
    if (pinAgain) {
      prompt = "ui.pin.again";
      pending = { ...p, done };
      return;
    }
    if (p.rest.length > 0 && needsApproval) {
      const [first, ...others] = chunks(p.rest, TICKET_USES);
      prompt = "ui.pin.prompt";
      pending = { op: p.op, jobs: first, rest: others.flat(), done };
      note = { tone: "ok", text: t("hub.games.found.more", { count: p.rest.length }) };
      await load();
      return;
    }
    pending = null;
    note = summary(p.op, done, failed);
    if (p.op !== "approve") selected = [];
    await load();
  }

  function summary(op: Pending["op"], done: number, failed: number) {
    if (op === "clear") return { tone: "ok" as const, text: t("hub.games.found.cleared") };
    if (op === "dismiss") {
      return failed > 0
        ? { tone: "refusal" as const, text: t("hub.settings.failed") }
        : { tone: "ok" as const, text: t("hub.games.found.dismissed_done") };
    }
    if (done === 0) return { tone: "refusal" as const, text: t("hub.games.found.failed") };
    return failed > 0
      ? { tone: "refusal" as const, text: t("hub.games.found.partial", { done, failed }) }
      : { tone: "ok" as const, text: t("hub.games.found.approved", { count: done }) };
  }

  async function approvePin(pin: string) {
    const current = pending;
    if (!current) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate(
      current.jobs.map((j) => ({ intent_kind: j.payload.kind })),
      pin,
    );
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    pending = null;
    void execute(current, outcome.ticket);
    return { ok: true } as const;
  }
</script>

<section class="found" aria-labelledby="found-title" data-testid="found">
  <h2 class="type-hub-title" id="found-title">{t("hub.games.found.title")}</h2>
  <p class="intro type-hub-body">{t("hub.games.found.intro")}</p>

  <p
    class="status type-hub-label"
    class:refusal={note?.tone === "refusal"}
    role="status"
    data-testid="found-status"
  >
    {#if note}
      {#if note.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{note.text}</span>
    {/if}
  </p>

  {#if pending}
    <div class="approval" data-testid="found-approval">
      <p class="type-hub-label">{t("hub.games.approves")}</p>
      <InlinePin message={prompt} onsubmit={approvePin} oncancel={() => (pending = null)} />
    </div>
  {/if}

  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.games.found.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else}
    {#if proposals.length === 0}
      <p class="type-hub-body" data-testid="found-empty">{t("hub.games.found.empty")}</p>
    {:else}
      <div class="bar">
        <label class="check type-hub-label"
          ><input
            type="checkbox"
            checked={allChosen}
            onchange={(e) => toggleAll(e.currentTarget.checked)}
            data-testid="found-select-all"
          />{t("hub.games.found.select_all")}</label
        >
        <button
          type="button"
          class="ui-interactive btn type-hub-label"
          disabled={chosen.length === 0 || busy}
          onclick={approveSelected}
          data-testid="found-approve"
          >{t("hub.games.found.approve", { count: chosen.length })}</button
        >
        <button
          type="button"
          class="ui-interactive btn type-hub-label"
          disabled={chosen.length === 0 || busy}
          onclick={dismissSelected}
          data-testid="found-dismiss"
          >{t("hub.games.found.dismiss", { count: chosen.length })}</button
        >
      </div>
      <ul class="rows" data-testid="found-list">
        {#each proposals as p (p.key)}
          <li class="row" data-testid="found-row" data-key={p.key}>
            <label class="check type-hub-label"
              ><input
                type="checkbox"
                checked={selected.includes(p.key)}
                onchange={(e) => toggle(p.key, e.currentTarget.checked)}
                aria-label={t("hub.games.found.select", { name: p.name })}
                data-testid="found-select"
              /></label
            >
            {#if p.cover_sha256 && covers[p.cover_sha256]}
              <img
                class="cover"
                src={covers[p.cover_sha256]}
                alt={t("hub.games.found.cover_alt", { name: p.name })}
                data-testid="found-cover"
              />
            {/if}
            <div class="info">
              <span class="type-hub-title" data-testid="found-name">{p.name}</span>
              <span class="type-hub-label" data-testid="found-kind">{kindText()}</span>
              <span class="type-hub-label" data-testid="found-launcher"
                >{launcherText(p.launcher)}</span
              >
              {#if p.category}<span class="type-hub-label" data-testid="found-category"
                  >{p.category}</span
                >{/if}
              <span class="badge type-hub-label" data-testid="found-match"
                >{p.catalog_id
                  ? t("hub.games.found.catalog")
                  : t("hub.games.found.unmatched")}</span
              >
              <span class="type-hub-label" data-testid="found-pcs"
                >{t("hub.games.found.pcs", { count: p.pc_count })}</span
              >
              <span class="type-hub-label path" data-testid="found-where"
                >{t("hub.games.found.where", {
                  names: p.devices.map((d) => d.label).join(", "),
                })}</span
              >
              {#if !p.catalog_id}<span class="type-hub-label path" data-testid="found-file"
                  >{p.file_name}</span
                >{/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
    {#if dismissed > 0}
      <div class="bar">
        <span class="type-hub-label" data-testid="found-dismissed"
          >{t("hub.games.found.dismissed", { count: dismissed })}</span
        >
        <button
          type="button"
          class="ui-interactive btn type-hub-label"
          disabled={busy}
          onclick={clearDismissed}
          data-testid="found-clear">{t("hub.games.found.clear")}</button
        >
      </div>
    {/if}
  {/if}
</section>

<style>
  .found {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h2,
  p {
    margin: 0;
  }

  .intro {
    color: var(--text-muted);
  }

  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
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

  .info {
    display: flex;
    flex: 1;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }

  .cover {
    inline-size: 4rem;
    block-size: 5.5rem;
    object-fit: cover;
    border-radius: var(--radius-md);
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

  .btn {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    cursor: pointer;
  }

  .btn:disabled {
    cursor: not-allowed;
    opacity: 0.6;
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
