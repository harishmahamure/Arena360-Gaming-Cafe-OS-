<script lang="ts">
  import { Button, InlinePin, formatDateTime, nextDay, t, zonedDayStart } from "@arena/ui";
  import type {
    AuditDetail,
    AuditFacets,
    AuditItem,
    AuditStaffOption,
    SessionStaff,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, dropElevation, elevate, hasPermission } from "../../../lib/auth";
  import {
    loadAuditFacets,
    loadAuditItem,
    loadAuditPage,
    type AuditFilters,
    type AuditOutcome,
  } from "../../../lib/audit";
  import AuditDetailPanel from "../../../lib/audit/AuditDetailPanel.svelte";
  import { actorName, KIND_LABELS, moduleLabel } from "../../../lib/audit/labels";
  import { branchLocale } from "../../../lib/locale.svelte";

  // The audit log, read only: filters, newest-first pages ("Load more", or the next page as
  // the end of the list scrolls into view) and a details panel. A member without
  // `audit.view` sees a manager PIN in place first; nothing is fetched until it is approved,
  // and the read ticket lives only in this page's memory (given up when the page closes).

  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });
  const canView = $derived(hasPermission(staff, "audit.view"));
  const tz = $derived(branchLocale.current?.tz);

  /** The read-elevation ticket (members without `audit.view`). */
  let ticket = $state<string | null>(null);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("hub.audit.needs_approval");

  let facets = $state<AuditFacets | null>(null);
  let items = $state<AuditItem[]>([]);
  let nextCursor = $state<string | null>(null);
  let loading = $state(false);
  let loaded = $state(false);
  /** The first page could not be loaded. */
  let loadFailed = $state(false);
  /** The next page could not be loaded (the rows already shown stay). */
  let moreFailed = $state(false);
  /** The From date is after the To date (checked before any request). */
  let rangeError = $state(false);
  /** The filter the Hub refused (`VALIDATION_FAILED` naming it). */
  let filterError = $state<string | null>(null);
  /** A call met the idle lock: reload once the session comes back. */
  let waitingForUnlock = false;
  /** Bumped by every new first-page load, so a stale answer is dropped. */
  let generation = 0;

  let actor = $state("");
  let module = $state("");
  let fromDay = $state("");
  let toDay = $state("");

  let selected = $state<AuditItem | null>(null);
  let detail = $state<AuditDetail | null>(null);
  let detailLoading = $state(false);
  let detailFailed = $state(false);
  /** The row button that opened the details (focus goes back to it on close). */
  let opener: HTMLElement | null = null;

  let sentinel: HTMLElement | undefined = $state();
  let askButton: HTMLElement | undefined = $state();
  const askEl = $derived(askButton?.querySelector("button") ?? null);

  const staffOptions = $derived<AuditStaffOption[]>(facets?.staff ?? []);

  function filters(): AuditFilters {
    const to = toDay ? nextDay(toDay) : null;
    return {
      actor,
      module,
      from: fromDay ? zonedDayStart(fromDay, tz) : null,
      to: to ? zonedDayStart(to, tz) : null,
    };
  }

  function askForApproval(prompt: MessageKey) {
    resetData();
    ticket = null;
    approvalPrompt = prompt;
    approving = true;
  }

  /**
   * Handles what every call may answer; `"stop"` when the caller should stop, `"failed"` for
   * an error the caller shows in its own place.
   */
  async function handled<T>(
    outcome: AuditOutcome<T>,
    listCall = true,
  ): Promise<"ok" | "stop" | "failed"> {
    if (outcome.kind === "ok") return "ok";
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return "stop";
    }
    if (outcome.kind === "locked") {
      waitingForUnlock = true; // the layout shows the lock
      return "stop";
    }
    if (outcome.kind === "denied") {
      // The approval lapsed or was dropped (a lock, a switch, the approver changed), or this
      // member lost audit.view: nothing stays on screen, and a manager's PIN can approve.
      if (canView) await invalidate(SESSION_DEPENDENCY);
      askForApproval(ticket ? "ui.pin.again" : "hub.audit.needs_approval");
      return "stop";
    }
    if (listCall && outcome.kind === "error" && outcome.error.code === "VALIDATION_FAILED") {
      filterError = outcome.error.field ?? "filters";
      return "stop";
    }
    return "failed";
  }

  function resetData() {
    generation++;
    items = [];
    nextCursor = null;
    loaded = false;
    loading = false;
    moreFailed = false;
    facets = null;
    void closeDetail(false);
  }

  /** The next page loads while the end of the list is in view (a short page included). */
  async function continueIfVisible() {
    await tick();
    const el = sentinel;
    if (!el || !nextCursor || loading || moreFailed) return;
    const rect = el.getBoundingClientRect();
    if (rect.top <= window.innerHeight && rect.bottom >= 0) void loadMore();
  }

  /** Loads the filter options (once) and the first page for the current filters. */
  async function reload() {
    if (!canView && !ticket) return;
    rangeError = Boolean(fromDay && toDay && fromDay > toDay);
    filterError = null;
    if (rangeError) return;
    const mine = ++generation;
    loading = true;
    loadFailed = false;
    moreFailed = false;
    void closeDetail(false);
    try {
      if (!facets) {
        const f = await loadAuditFacets(ticket);
        if (mine !== generation) return;
        const how = await handled(f);
        if (how === "failed") loadFailed = true;
        if (how !== "ok") return;
        if (f.kind === "ok") facets = f.value;
      }
      const result = await loadAuditPage(filters(), null, ticket);
      if (mine !== generation) return;
      const how = await handled(result);
      if (how === "failed") loadFailed = true;
      if (how !== "ok" || result.kind !== "ok") return;
      items = result.value.items;
      nextCursor = result.value.next_cursor;
      loaded = true;
    } finally {
      if (mine === generation) loading = false;
    }
    if (mine === generation) void continueIfVisible();
  }

  async function loadMore() {
    if (loading || !nextCursor) return;
    const mine = generation;
    loading = true;
    moreFailed = false;
    let ok = false;
    try {
      const result = await loadAuditPage(filters(), nextCursor, ticket);
      if (mine !== generation) return;
      const how = await handled(result);
      if (how === "failed") moreFailed = true;
      if (how !== "ok" || result.kind !== "ok") return;
      items = [...items, ...result.value.items];
      nextCursor = result.value.next_cursor;
      ok = true;
    } finally {
      if (mine === generation) loading = false;
    }
    if (ok && mine === generation) void continueIfVisible();
  }

  function clearFilters() {
    actor = "";
    module = "";
    fromDay = "";
    toDay = "";
    void reload();
  }

  async function openDetail(item: AuditItem, from: HTMLElement | null) {
    opener = from;
    selected = item;
    detail = null;
    detailFailed = false;
    detailLoading = true;
    try {
      const result = await loadAuditItem(item.epoch, item.seq, ticket);
      if (selected !== item) return;
      const how = await handled(result, false);
      if (how === "failed") detailFailed = true;
      if (how === "ok" && result.kind === "ok") detail = result.value;
    } finally {
      if (selected === item) detailLoading = false;
    }
  }

  async function closeDetail(refocus = true) {
    selected = null;
    detail = null;
    detailLoading = false;
    detailFailed = false;
    if (refocus && opener) {
      const el = opener;
      await tick();
      el.focus();
    }
  }

  /** A manager's PIN approves reading the audit log for a while. */
  async function approve(pin: string) {
    const outcome = await elevate([{ permission: "audit.view" }], pin);
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind === "unreachable")
      return { ok: false, message: "ui.pin.unreachable" } as const;
    if (outcome.kind === "error") return { ok: false, message: "ui.pin.failed" } as const;
    if (!approving) {
      // Cancelled while the PIN was checked: nothing is shown.
      void dropElevation(outcome.ticket);
      return { ok: true } as const;
    }
    ticket = outcome.ticket;
    approving = false;
    void reload();
    return { ok: true } as const;
  }

  function cancelApproval() {
    approving = false;
  }

  // The member changed (an unlock with another PIN): start again with their rights. The same
  // member came back (an unlock): load what a lock interrupted.
  let seenStaff = untrack(() => staff);
  $effect(() => {
    const now = staff;
    const allowed = canView;
    untrack(() => {
      if (now === seenStaff) return;
      const switched = (now?.staff_id ?? null) !== (seenStaff?.staff_id ?? null);
      seenStaff = now;
      if (switched) {
        if (ticket) void dropElevation(ticket);
        if (allowed) {
          resetData();
          ticket = null;
          approving = false;
          void reload();
        } else askForApproval("hub.audit.needs_approval");
        return;
      }
      const resume = waitingForUnlock || !loaded;
      waitingForUnlock = false;
      if (resume && !approving && !loading && (allowed || ticket)) void reload();
    });
  });

  // The next page loads as the end of the list scrolls into view.
  $effect(() => {
    const el = sentinel;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting) && nextCursor && !loading && !moreFailed) {
        void loadMore();
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  });

  onMount(() => {
    if (canView) void reload();
    else approving = true;
  });

  onDestroy(() => {
    if (ticket) void dropElevation(ticket);
  });

  const invalid = (field: string) => filterError === field;
</script>

<section class="audit" aria-labelledby="audit-title" data-testid="audit">
  <h2 class="type-hub-title" id="audit-title">{t("hub.audit.title")}</h2>
  <p class="intro type-hub-body">{t("hub.audit.intro")}</p>

  {#if approving}
    <div class="approval" data-testid="audit-approval">
      <InlinePin
        message={approvalPrompt}
        onsubmit={approve}
        oncancel={cancelApproval}
        returnFocusTo={askEl}
      />
    </div>
  {:else if !canView && !ticket}
    <div class="actions" data-testid="audit-approval-cancelled">
      <p class="note type-hub-label">{t("hub.audit.approval_cancelled")}</p>
      <span bind:this={askButton}>
        <Button
          label="hub.audit.ask_approval"
          variant="secondary"
          onclick={() => {
            approvalPrompt = "hub.audit.needs_approval";
            approving = true;
          }}
        />
      </span>
    </div>
  {:else}
    <form
      class="filters"
      aria-label={t("hub.audit.filters")}
      data-testid="audit-filters"
      onsubmit={(e) => e.preventDefault()}
    >
      <div class="field">
        <label class="label type-hub-label" for="audit-actor">{t("hub.audit.filter.actor")}</label>
        <select
          id="audit-actor"
          class="control ui-interactive"
          class:invalid={invalid("actor")}
          aria-invalid={invalid("actor") ? "true" : undefined}
          bind:value={actor}
          onchange={() => void reload()}
        >
          <option value="">{t("hub.audit.filter.actor_all")}</option>
          {#each staffOptions as s (s.id)}
            <!-- i18n-ignore -->
            <option value={"staff:" + s.id}>{s.name}</option>
          {/each}
          <option value="system">{t("hub.audit.actor.system")}</option>
          <option value="owner">{t("hub.audit.actor.owner")}</option>
          <option value="device">{t("hub.audit.filter.devices")}</option>
          <option value="admin">{t("hub.audit.actor.admin")}</option>
        </select>
      </div>
      <div class="field">
        <label class="label type-hub-label" for="audit-module">{t("hub.audit.filter.module")}</label
        >
        <select
          id="audit-module"
          class="control ui-interactive"
          class:invalid={invalid("module")}
          aria-invalid={invalid("module") ? "true" : undefined}
          bind:value={module}
          onchange={() => void reload()}
        >
          <option value="">{t("hub.audit.filter.module_all")}</option>
          {#each facets?.modules ?? [] as m (m)}
            <option value={m}>{moduleLabel(m)}</option>
          {/each}
        </select>
      </div>
      <div class="field">
        <label class="label type-hub-label" for="audit-from">{t("hub.audit.filter.from")}</label>
        <input
          id="audit-from"
          class="control ui-interactive"
          class:invalid={rangeError || invalid("from")}
          aria-invalid={rangeError || invalid("from") ? "true" : undefined}
          aria-describedby="audit-filter-error"
          type="date"
          bind:value={fromDay}
          max={toDay || undefined}
          onchange={() => void reload()}
        />
      </div>
      <div class="field">
        <label class="label type-hub-label" for="audit-to">{t("hub.audit.filter.to")}</label>
        <input
          id="audit-to"
          class="control ui-interactive"
          class:invalid={rangeError || invalid("to")}
          aria-invalid={rangeError || invalid("to") ? "true" : undefined}
          aria-describedby="audit-filter-error"
          type="date"
          bind:value={toDay}
          min={fromDay || undefined}
          onchange={() => void reload()}
        />
      </div>
      <div class="field clear">
        <Button label="hub.audit.filter.clear" variant="secondary" onclick={clearFilters} />
      </div>
      <p
        class="error type-hub-label"
        id="audit-filter-error"
        aria-live="polite"
        data-testid="audit-filter-error"
      >
        {#if rangeError}<span aria-hidden="true">⚠</span>
          <span>{t("hub.audit.filter.range_invalid")}</span>
        {:else if filterError}<span aria-hidden="true">⚠</span>
          <span>{t("hub.audit.filter.invalid")}</span>{/if}
      </p>
    </form>

    <div class="body" class:with-detail={selected !== null}>
      <div class="list">
        {#if loadFailed}
          <div class="actions" data-testid="audit-load-failed">
            <p class="status refusal type-hub-label" role="status">
              <span aria-hidden="true">⚠</span>
              <span>{t("hub.audit.load_failed")}</span>
            </p>
            <Button label="common.retry" variant="secondary" onclick={() => void reload()} />
          </div>
        {:else if loaded && items.length === 0}
          <p class="empty type-hub-body" role="status" data-testid="audit-empty">
            {t("hub.audit.empty")}
          </p>
        {:else if items.length > 0}
          <div class="scroll">
            <table class="rows type-hub-label" data-testid="audit-table">
              <caption class="visually-hidden">{t("hub.audit.table")}</caption>
              <thead>
                <tr>
                  <th scope="col">{t("hub.audit.col.time")}</th>
                  <th scope="col">{t("hub.audit.col.actor")}</th>
                  <th scope="col">{t("hub.audit.col.module")}</th>
                  <th scope="col">{t("hub.audit.col.intent")}</th>
                  <th scope="col">{t("hub.audit.col.record")}</th>
                </tr>
              </thead>
              <tbody>
                {#each items as item (item.epoch + "/" + item.seq)}
                  {@const time = formatDateTime(item.at, tz)}
                  <tr
                    class:selected={selected === item}
                    data-testid="audit-row"
                    onclick={(e) =>
                      void openDetail(
                        item,
                        (e.currentTarget as HTMLElement).querySelector("button"),
                      )}
                  >
                    <td>
                      <button
                        type="button"
                        class="open ui-interactive tabular-nums"
                        aria-label={t("hub.audit.open", { seq: item.seq, time })}
                        aria-pressed={selected === item}
                        onclick={(e) => {
                          e.stopPropagation();
                          void openDetail(item, e.currentTarget);
                        }}>{time}</button
                      >
                    </td>
                    <td>
                      <span class="name">{actorName(item.actor)}</span>
                      <span class="kind">{t(KIND_LABELS[item.actor.kind])}</span>
                    </td>
                    <td>{moduleLabel(item.module)}</td>
                    <td>{item.intent_kind}</td>
                    <td class="record">{item.tbl} / {item.row_id}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
        <div class="more">
          {#if moreFailed}
            <p class="status refusal type-hub-label" role="status" data-testid="audit-more-failed">
              <span aria-hidden="true">⚠</span>
              <span>{t("hub.audit.load_more_failed")}</span>
            </p>
            <Button label="common.retry" variant="secondary" onclick={() => void loadMore()} />
          {:else if nextCursor && !loadFailed}
            <Button
              label="hub.audit.load_more"
              variant="secondary"
              disabled={loading}
              disabledReason="hub.audit.loading"
              onclick={() => void loadMore()}
            />
          {:else if loaded && items.length > 0}
            <p class="note type-hub-label">{t("hub.audit.loaded_all")}</p>
          {/if}
          {#if loading}
            <p class="note type-hub-label" role="status">{t("hub.audit.loading")}</p>
          {/if}
        </div>
        <div class="sentinel" bind:this={sentinel} aria-hidden="true"></div>
      </div>
      {#if selected}
        {#if detail}
          <AuditDetailPanel {detail} {tz} onclose={() => void closeDetail()} />
        {:else if detailFailed}
          <div class="actions" data-testid="audit-detail-failed">
            <p class="status refusal type-hub-label" role="status">
              <span aria-hidden="true">⚠</span>
              <span>{t("hub.audit.detail.load_failed")}</span>
            </p>
            <Button
              label="common.retry"
              variant="secondary"
              onclick={() => selected && void openDetail(selected, opener)}
            />
            <Button
              label="hub.audit.detail.close"
              variant="secondary"
              onclick={() => void closeDetail()}
            />
          </div>
        {:else if detailLoading}
          <p class="note type-hub-label" role="status">{t("hub.audit.detail.loading")}</p>
        {/if}
      {/if}
    </div>
  {/if}
</section>

<style>
  .audit {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  .intro,
  .note,
  .empty,
  .status {
    margin: 0;
  }

  .intro,
  .note {
    color: var(--text-muted);
  }

  .approval {
    max-width: 32rem;
  }

  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .label {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .control {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
  }

  .control.invalid {
    border-width: 2px;
    border-color: var(--danger);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .error {
    display: flex;
    flex-basis: 100%;
    gap: var(--space-2);
    min-height: 1lh;
    margin: 0;
    color: var(--text-field-error-text);
  }

  .control:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .body {
    display: grid;
    gap: var(--space-4);
    align-items: start;
  }

  @media (min-width: 1100px) {
    .body.with-detail {
      grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
    }
  }

  .list {
    display: grid;
    gap: var(--space-3);
    min-width: 0;
  }

  .scroll {
    overflow-x: auto;
  }

  .rows {
    width: 100%;
    border-collapse: collapse;
  }

  .rows th,
  .rows td {
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--border-strong);
    text-align: left;
    vertical-align: middle;
  }

  .rows thead th {
    color: var(--text-muted);
  }

  .rows tbody tr {
    cursor: pointer;
  }

  /* The open row: a heavier name and the pressed button say so (never colour alone). */
  .rows tr.selected td {
    font-weight: var(--type-hub-title-weight);
  }

  .open {
    --ui-bg: transparent;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    padding: 0 var(--space-2);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .open[aria-pressed="true"] {
    border-color: var(--border-strong);
  }

  .name {
    display: block;
    color: var(--text);
  }

  .kind {
    display: block;
    color: var(--text-muted);
  }

  .record {
    overflow-wrap: anywhere;
  }

  .actions,
  .more {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .status {
    display: flex;
    gap: var(--space-2);
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }

  .sentinel {
    height: 1px;
  }
</style>
