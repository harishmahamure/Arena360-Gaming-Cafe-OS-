<script lang="ts">
  import { Button, Card, t } from "@arena/ui";
  import type { AuditItem, AuditSearchPage, OwnerBranch } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy } from "svelte";
  import {
    ACTOR_KINDS,
    AUDIT_POLL_MS,
    BY,
    MODULES,
    actorName,
    buildAudit,
    elevation,
    intentName,
    moduleName,
    opLabel,
    queryProblem,
    searchAudit,
    type AuditAnswer,
    type AuditQuery,
  } from "./audit";
  import { dateIn } from "./reports";
  import { refresh } from "./shell.svelte";

  let { branches, initial, now }: { branches: OwnerBranch[]; initial: string; now: number } =
    $props();

  // svelte-ignore state_referenced_locally
  let branchId = $state(branches.some((b) => b.id === initial) ? initial : (branches[0]?.id ?? ""));
  const branch = $derived(branches.find((b) => b.id === branchId) ?? branches[0]);
  const thisMonth = $derived(dateIn(branch?.timezone ?? "UTC", now).slice(0, 7));
  // svelte-ignore state_referenced_locally
  let month = $state(dateIn(branch?.timezone ?? "UTC", now).slice(0, 7));
  let query: AuditQuery = $state({ by: "recent", actorKind: "staff", text: "", module: "session" });

  // What the results area shows: nothing yet, a month being prepared, a failed build, an error,
  // or the pages of a search.
  type View =
    | { kind: "idle" }
    | { kind: "preparing"; progress: number }
    | { kind: "failed" }
    | { kind: "error"; message: MessageKey }
    | { kind: "results" };
  let view: View = $state({ kind: "idle" });
  let problem: MessageKey | null = $state(null);
  let items: AuditItem[] = $state([]);
  let orphaned: AuditItem[] = $state([]);
  let cursor: string | null = $state(null);
  let more = $state(false);
  let moreFailed = $state(false);
  let announcement = $state("");
  // The search the results belong to; "Load more" and polling keep using it.
  let ran: { branchId: string; month: string; query: AuditQuery } | null = $state(null);
  let ticket = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const stop = () => {
    clearTimeout(timer);
    timer = undefined;
  };
  onDestroy(() => {
    ticket++;
    stop();
  });

  // Rows are shown in the zone of the branch that was searched, not the one now picked.
  const zone = $derived(
    branches.find((b) => b.id === (ran?.branchId ?? branchId))?.timezone ?? branch?.timezone,
  );
  const time = (ms: number) => {
    const options = {
      day: "2-digit",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    } as const;
    try {
      return new Intl.DateTimeFormat("en-GB", {
        ...options,
        timeZone: zone ?? "UTC",
      }).format(ms);
    } catch {
      return new Intl.DateTimeFormat("en-GB", { ...options, timeZone: "UTC" }).format(ms);
    }
  };

  function show(page: AuditSearchPage, append: boolean) {
    items = append ? [...items, ...page.items] : page.items;
    if (!append) orphaned = page.orphaned;
    cursor = page.next_cursor;
    view = { kind: "results" };
    announcement = "";
  }

  // One step: ask, then act on the answer. A month that is not ready is asked about again
  // every few seconds until its first page arrives; a month never built is started first.
  async function step(mine: number, answer: AuditAnswer, built = false) {
    if (mine !== ticket) return;
    if (answer.state === "signed-out") {
      await refresh();
      return;
    }
    if (answer.state === "page") return show(answer.page, false);
    if (answer.state === "failed") {
      view = { kind: "failed" };
      announcement = "";
      return;
    }
    if (answer.state === "error") {
      view = { kind: "error", message: answer.message };
      return;
    }
    if (answer.state === "missing") {
      // A month still missing right after its build was started is not coming: stop, don't loop.
      if (built || !ran) {
        view = { kind: "error", message: "owner.audit.error.generic" };
        return;
      }
      return step(mine, await buildAudit(ran.branchId, ran.month), true);
    }
    if (view.kind !== "preparing") announcement = t("owner.audit.preparing");
    view = { kind: "preparing", progress: answer.progress };
    timer = setTimeout(async () => {
      if (mine !== ticket || !ran) return;
      void step(mine, await searchAudit(ran.branchId, ran.month, ran.query));
    }, AUDIT_POLL_MS);
  }

  async function search() {
    problem = queryProblem(query);
    if (!problem && (!/^\d{4}-\d{2}$/.test(month) || month > thisMonth))
      problem = "owner.audit.problem.month";
    if (problem || !branch) return;
    stop();
    const mine = ++ticket;
    ran = { branchId, month, query: { ...query } };
    items = [];
    orphaned = [];
    cursor = null;
    moreFailed = false;
    more = false;
    view = { kind: "preparing", progress: 0 };
    await step(mine, await searchAudit(branchId, month, query));
  }

  // Retry after a failed build starts a new attempt.
  async function retry() {
    if (!ran) return;
    stop();
    const mine = ++ticket;
    view = { kind: "preparing", progress: 0 };
    await step(mine, await buildAudit(ran.branchId, ran.month));
  }

  async function loadMore() {
    if (!ran || !cursor || more) return;
    const mine = ticket;
    more = true;
    moreFailed = false;
    const answer = await searchAudit(ran.branchId, ran.month, ran.query, cursor);
    more = false;
    if (mine !== ticket) return;
    if (answer.state === "page") show(answer.page, true);
    else if (answer.state === "signed-out") await refresh();
    else moreFailed = true;
  }

  const record = (i: AuditItem) =>
    t("owner.audit.record", { op: t(opLabel(i.op)), table: i.tbl, id: i.row_id });
</script>

{#snippet rows(list: AuditItem[], caption: MessageKey)}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrolling table must be reachable by keyboard) -->
  <div class="scroll" role="region" aria-label={t(caption)} tabindex="0">
    <table class="table">
      <caption class="visually-hidden">{t(caption)}</caption>
      <thead>
        <tr>
          <th scope="col">{t("hub.audit.col.time")}</th>
          <th scope="col">{t("hub.audit.col.actor")}</th>
          <th scope="col">{t("hub.audit.col.intent")}</th>
          <th scope="col">{t("hub.audit.col.record")}</th>
        </tr>
      </thead>
      <tbody>
        {#each list as item (item.epoch + ":" + item.seq)}
          {@const approved = elevation(item.elevated_by)}
          <tr data-seq={item.seq}>
            <td class="tabular-nums">{time(item.at)}</td>
            <td>
              <span class="name">{actorName(item.actor)}</span>
              {#if item.actor.kind === "device" || item.actor.kind === "staff"}
                <span class="note type-hub-label"
                  >{t(
                    item.actor.kind === "staff" ? "hub.audit.kind.staff" : "hub.audit.kind.device",
                  )}</span
                >
              {/if}
            </td>
            <td>
              <span>{intentName(item.intent_kind)}</span>
              <span class="note type-hub-label">{moduleName(item.module)}</span>
              {#if approved}<span class="note type-hub-label">{approved}</span>{/if}
            </td>
            <td class="record">{record(item)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/snippet}

<Card>
  <form
    class="form"
    aria-label={t("owner.audit.form")}
    onsubmit={(e) => {
      e.preventDefault();
      void search();
    }}
  >
    <p class="muted">{t("owner.audit.intro")}</p>
    <div class="controls">
      {#if branches.length > 1}
        <div class="field">
          <label class="label" for="aud-branch">{t("owner.audit.branch")}</label>
          <select id="aud-branch" class="ui-interactive select" bind:value={branchId}>
            {#each branches as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
          </select>
        </div>
      {/if}
      <div class="field">
        <label class="label" for="aud-month">{t("owner.audit.month")}</label>
        <input
          id="aud-month"
          class="ui-interactive select"
          type="month"
          max={thisMonth}
          bind:value={month}
        />
      </div>
      <div class="field">
        <label class="label" for="aud-by">{t("owner.audit.by")}</label>
        <select
          id="aud-by"
          class="ui-interactive select"
          bind:value={query.by}
          onchange={() => (query.text = "")}
        >
          {#each BY as [id, label] (id)}<option value={id}>{t(label)}</option>{/each}
        </select>
      </div>
      {#if query.by === "actor"}
        <div class="field">
          <label class="label" for="aud-actor">{t("owner.audit.actor")}</label>
          <select id="aud-actor" class="ui-interactive select" bind:value={query.actorKind}>
            {#each ACTOR_KINDS as [id, label] (id)}<option value={id}>{t(label)}</option>{/each}
          </select>
        </div>
      {:else if query.by === "module"}
        <div class="field">
          <label class="label" for="aud-module">{t("owner.audit.module")}</label>
          <select id="aud-module" class="ui-interactive select" bind:value={query.module}>
            {#each MODULES as [id, label] (id)}<option value={id}>{t(label)}</option>{/each}
          </select>
        </div>
      {/if}
      {#if query.by === "actor" || query.by === "intent" || query.by === "player"}
        <div class="field">
          <label class="label" for="aud-text">
            {t(
              query.by === "actor"
                ? "owner.audit.actor_id"
                : query.by === "intent"
                  ? "owner.audit.intent"
                  : "owner.audit.player",
            )}
          </label>
          <input
            id="aud-text"
            class="ui-interactive select"
            type="text"
            autocomplete="off"
            spellcheck="false"
            bind:value={query.text}
          />
        </div>
      {/if}
    </div>
    {#if problem}<p role="alert">{t(problem)}</p>{/if}
    <div>
      <Button label="owner.audit.search" type="submit" />
    </div>
  </form>
</Card>

<div class="visually-hidden" role="status" aria-live="polite">{announcement}</div>

<div class="results" aria-busy={view.kind === "preparing"}>
  {#if view.kind === "preparing"}
    <div class="preparing" data-testid="audit-preparing">
      <p class="type-hub-title">{t("owner.audit.preparing")}</p>
      <p class="muted">{t("owner.audit.preparing.progress", { n: Math.round(view.progress) })}</p>
      <progress max="100" value={view.progress} aria-label={t("owner.audit.preparing")}></progress>
    </div>
  {:else if view.kind === "failed"}
    <div class="preparing" role="alert">
      <p>{t("owner.audit.failed")}</p>
      <Button label="common.retry" variant="secondary" onclick={retry} />
    </div>
  {:else if view.kind === "error"}
    <p role="alert">{t(view.message)}</p>
  {:else if view.kind === "results"}
    {#if items.length}
      {@render rows(items, "owner.audit.table")}
    {:else}
      <p class="muted" role="status">{t("hub.audit.empty")}</p>
    {/if}
    {#if moreFailed}<p role="alert">{t("owner.audit.load_more_failed")}</p>{/if}
    {#if cursor}
      <div>
        <Button
          label="hub.audit.load_more"
          variant="secondary"
          onclick={loadMore}
          disabled={more}
          disabledReason="hub.audit.loading"
        />
      </div>
    {:else if items.length}
      <p class="muted type-hub-label">{t("hub.audit.loaded_all")}</p>
    {/if}
    {#if orphaned.length}
      <section aria-labelledby="aud-orphaned" data-testid="audit-orphaned">
        <h2 id="aud-orphaned" class="type-hub-title">{t("owner.audit.orphaned")}</h2>
        <p class="muted">{t("owner.audit.orphaned.note")}</p>
        {@render rows(orphaned, "owner.audit.orphaned")}
      </section>
    {/if}
  {/if}
</div>

<style>
  .form {
    display: grid;
    gap: var(--space-3);
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .label {
    font-weight: var(--type-hub-title-weight);
  }
  .select {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
  }
  .muted,
  .note {
    color: var(--text-muted);
  }
  .note {
    display: block;
  }
  .results {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--space-4);
    margin-top: var(--space-4);
  }
  .preparing {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-4);
    background-color: var(--surface);
    border-radius: var(--radius-sm);
  }
  .preparing p {
    margin: 0;
  }
  .preparing .muted {
    color: var(--text-muted);
  }
  progress {
    width: 100%;
  }
  .scroll {
    overflow-x: auto;
  }
  .table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    padding: var(--space-2) var(--space-3);
    text-align: start;
    vertical-align: top;
    border-bottom: 1px solid var(--border-strong);
  }
  .record {
    overflow-wrap: anywhere;
  }
  h2 {
    margin: var(--space-4) 0 var(--space-2);
  }
</style>
