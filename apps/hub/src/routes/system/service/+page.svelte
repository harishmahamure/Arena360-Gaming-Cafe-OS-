<script lang="ts">
  import { Button, InlinePin, StatusBadge, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SessionStaff } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    elevate,
    hasPermission,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { bridge, refreshAccess, service } from "../../../lib/service.svelte";

  const KEY = "hub.stopped_at";

  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });
  const canStop = $derived(hasPermission(staff, "settings.edit"));

  let confirming = $state(false);
  let approving = $state(false);
  let busy = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let idempotencyKey = newIdempotencyKey();

  onMount(() => {
    refreshAccess();
    const onFocus = () => refreshAccess();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  async function stop(ticket?: string) {
    if (busy) return;
    busy = true;
    status = null;
    try {
      const loaded = await loadSettings();
      if (loaded.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const rev = loaded.kind === "ok" ? loaded.settings.find((s) => s.key === KEY)?.rev : null;
      if (rev == null) {
        status = { tone: "refusal", message: "hub.service.stop_failed" };
        return;
      }
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY, value: String(Date.now()) },
        idempotencyKey,
        rev,
        { elevationTicket: ticket },
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind !== "applied") {
        const denied = outcome.kind === "error" && outcome.error.code === "PERMISSION_DENIED";
        status = {
          tone: "refusal",
          message: denied ? "hub.refusal.permission" : "hub.service.stop_failed",
        };
        if (!denied) idempotencyKey = newIdempotencyKey();
        return;
      }
      status = { tone: "ok", message: "hub.service.stopped" };
      bridge()?.stopHub();
    } finally {
      busy = false;
      confirming = false;
    }
  }

  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: "settings.set", key: KEY }], pin);
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
    void stop(outcome.ticket);
    return { ok: true } as const;
  }

  function begin() {
    status = null;
    if (canStop) confirming = true;
    else approving = true;
  }
</script>

<section class="service" aria-labelledby="service-title">
  <h2 class="type-hub-title" id="service-title">{t("hub.service.title")}</h2>
  <p class="type-hub-body">{t("hub.service.intro")}</p>

  {#if service.access}
    <div class="row" data-testid="service-battery">
      {#if service.access.battery_exempt}
        <p class="type-hub-label">{t("hub.service.battery_off")}</p>
      {:else}
        <StatusBadge state="warning" label="hub.service.battery_on" />
        <p class="type-hub-body">{t("hub.service.battery_help")}</p>
        <Button
          label="hub.service.battery_button"
          variant="secondary"
          onclick={() => bridge()?.openBatterySettings()}
        />
      {/if}
    </div>
    {#if !service.access.notifications}
      <div class="row" data-testid="service-notifications">
        <StatusBadge state="warning" label="hub.service.notifications_off" />
        <p class="type-hub-body">{t("hub.service.notifications_help")}</p>
        <Button
          label="hub.service.notifications_button"
          variant="secondary"
          onclick={() => bridge()?.openNotificationSettings()}
        />
      </div>
    {/if}

    <h3 class="type-hub-label">{t("hub.service.stop")}</h3>
    <p class="type-hub-body">{t("hub.service.stop_help")}</p>
    <p class="status type-hub-label" role="status" data-testid="service-status">
      {#if status}
        {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
        <span>{t(status.message)}</span>
      {/if}
    </p>
    {#if approving}
      <div class="approval" data-testid="service-approval">
        <p class="type-hub-label">{t("hub.service.approves")}</p>
        <InlinePin onsubmit={approve} oncancel={() => (approving = false)} />
      </div>
    {:else if confirming}
      <div class="actions">
        <Button
          label="hub.service.stop_confirm"
          disabled={busy}
          disabledReason="hub.service.stopped"
          onclick={() => void stop()}
        />
        <Button
          label="hub.service.cancel"
          variant="secondary"
          onclick={() => (confirming = false)}
        />
      </div>
    {:else}
      <Button label="hub.service.stop" variant="secondary" onclick={begin} />
    {/if}
  {:else}
    <p class="type-hub-body" data-testid="service-unavailable">{t("hub.service.unavailable")}</p>
  {/if}
</section>

<style>
  .service {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  .row,
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .status {
    min-height: 1lh;
  }
</style>
