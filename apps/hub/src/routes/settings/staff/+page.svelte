<script lang="ts">
  import { Button, InlinePin, TierLock, t } from "@arena/ui";
  import type { StaffList, StaffView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, hasPermission } from "../../../lib/auth";
  import { sessionStaff } from "../../../lib/operating";
  import { deactivateStaff, loadStaff } from "../../../lib/staff";

  /** The built-in Owner role, the only one that deactivates. */
  const OWNER_ROLE = "Owner";

  let list = $state<StaffList | null>(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let denied = $state(false);
  let target = $state<StaffView | null>(null);
  let status: { tone: "ok" | "refusal"; text: string } | null = $state(null);

  const me = $derived(sessionStaff(page.data));
  const canSee = $derived(hasPermission(me, "user.manage"));
  const isOwner = $derived(me?.role_name === OWNER_ROLE);
  const full = $derived(list !== null && list.used >= list.limit);
  /** The tier whose limit lifts this one: Basic from Free, Premium above that. */
  const nextTier = $derived(list !== null && list.limit >= 10 ? "Premium" : "Basic");

  async function load() {
    const result = await loadStaff();
    if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    denied = result.kind === "denied";
    loadFailed = result.kind === "unreachable";
    if (result.kind === "ok") list = result.list;
    loading = false;
  }

  onMount(() => void load());

  async function confirm(pin: string) {
    const who = target;
    if (!who) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await deactivateStaff(who.id, pin);
    switch (outcome.kind) {
      case "signed_out":
        await invalidate(SESSION_DEPENDENCY);
        return { ok: false, message: "ui.pin.unreachable" } as const;
      case "rejected":
        return { ok: false, message: "hub.staff.pin_rejected" } as const;
      case "locked":
        return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
      case "unreachable":
        return { ok: false, message: "ui.pin.unreachable" } as const;
      case "refused": {
        target = null;
        const message: MessageKey =
          outcome.field === "staff_id" ? "hub.staff.refused.staff_id" : "hub.staff.refused";
        status = { tone: "refusal", text: t(message) };
        void load();
        return { ok: true } as const;
      }
      case "done":
        target = null;
        status = { tone: "ok", text: t("hub.staff.deactivated", { name: who.name }) };
        void load();
        return { ok: true } as const;
    }
  }
</script>

<section class="staff" data-testid="staff-page">
  <h2 class="type-hub-title">{t("hub.staff.title")}</h2>
  <p class="intro type-hub-body">{t("hub.staff.intro")}</p>

  {#if loading}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else if denied || !canSee}
    <p class="type-hub-body" role="alert" data-testid="staff-no-access">
      {t("hub.staff.no_access")}
    </p>
  {:else if loadFailed || list === null}
    <p class="type-hub-body" role="alert">{t("hub.staff.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else}
    <p class="type-hub-label" data-testid="staff-count">
      {t("hub.staff.count", { used: list.used, limit: list.limit })}
    </p>
    {#if full}
      <div role="alert" data-testid="staff-limit">
        <p class="type-hub-label">{t("hub.staff.limit")}</p>
        <TierLock tier={nextTier} />
      </div>
    {/if}

    <div
      class="status type-hub-label"
      class:refusal={status?.tone === "refusal"}
      role="status"
      data-testid="staff-status"
    >
      {#if status}
        {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
        <span>{status.text}</span>
      {/if}
    </div>

    {#if target}
      <div class="approval" data-testid="staff-approval">
        <p class="type-hub-label">{t("hub.staff.deactivating", { name: target.name })}</p>
        <InlinePin
          message="hub.staff.pin_prompt"
          label="hub.staff.pin_label"
          submitLabel="hub.staff.pin_submit"
          onsubmit={confirm}
          oncancel={() => (target = null)}
        />
      </div>
    {/if}

    {#if list.staff.length === 0}
      <p class="type-hub-body">{t("hub.staff.empty")}</p>
    {:else}
      <ul class="rows" aria-label={t("hub.staff.list")} data-testid="staff-list">
        {#each list.staff as m (m.id)}
          <li class="row" data-testid="staff-row" data-staff-id={m.id}>
            <span class="type-hub-title">{m.name}</span>
            <span class="type-hub-label">{m.role}</span>
            <span class="badge type-hub-label" data-testid="staff-status-badge"
              >{t(
                m.status === "invited" ? "hub.staff.status.invited" : "hub.staff.status.active",
              )}</span
            >
            {#if isOwner && m.id !== me?.staff_id}
              <span class="actions">
                <Button
                  label="hub.staff.deactivate"
                  variant="secondary"
                  onclick={() => {
                    status = null;
                    target = m;
                  }}
                />
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .staff {
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

  .badge {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
  }

  .actions {
    margin-inline-start: auto;
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
