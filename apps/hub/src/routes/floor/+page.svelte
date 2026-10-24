<script lang="ts">
  import { Button, InlinePin, StatusBadge, t } from "@arena/ui";
  import type { SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { tick } from "svelte";
  import { waitForAck } from "../../lib/admin-unlock";
  import {
    SESSION_DEPENDENCY,
    dropElevation,
    elevate,
    hasPermission,
    newIdempotencyKey,
    submitIntent,
  } from "../../lib/auth";
  import type { PageData } from "./$types";

  let { data }: { data: PageData } = $props();

  /**
   * Remote unlock (Story 1.20): `device.admin_unlock` for an online paired kiosk, approved by a
   * manager's PIN in place when the member lacks `device.remote`. The Hub pushes it to the kiosk;
   * the floor waits up to 5 s for the kiosk's acknowledgement, and says "Failed — retry"
   * otherwise (at once for an offline device).
   */
  type UnlockState = "waiting" | "done" | "failed";
  let unlocks: Record<string, UnlockState> = $state({});
  /** The device whose unlock waits for a manager's PIN. */
  let approving: string | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  const wraps: Record<string, HTMLElement | undefined> = {};

  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });
  const needsApproval = $derived(!hasPermission(staff, "device.remote"));

  const buttonOf = (id: string) => wraps[id]?.querySelector("button") ?? null;

  async function unlock(deviceId: string, online: boolean, ticket?: string) {
    if (!online) {
      if (ticket) void dropElevation(ticket);
      unlocks[deviceId] = "failed";
      return;
    }
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = deviceId;
      return;
    }
    unlocks[deviceId] = "waiting";
    const outcome = await submitIntent(
      { kind: "device.admin_unlock", device_id: deviceId },
      newIdempotencyKey(),
      undefined,
      { elevationTicket: ticket },
    );
    if (outcome.kind === "signed_out") {
      delete unlocks[deviceId];
      return void (await invalidate(SESSION_DEPENDENCY));
    }
    if (outcome.kind === "error" && outcome.error.code === "PIN_REQUIRED") {
      delete unlocks[deviceId];
      approvalPrompt = "ui.pin.again";
      approving = deviceId;
      return;
    }
    const exitId =
      outcome.kind === "applied"
        ? outcome.result.applied.changes.find((c) => c.tbl === "device_admin_exits")?.row_id
        : undefined;
    unlocks[deviceId] = exitId && (await waitForAck(exitId)) ? "done" : "failed";
  }

  async function approve(pin: string) {
    const deviceId = approving;
    const outcome = await elevate([{ intent_kind: "device.admin_unlock" }], pin);
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
    if (approving !== deviceId || deviceId === null) {
      void dropElevation(outcome.ticket);
      return { ok: true } as const;
    }
    approving = null;
    const device = snapshot?.devices.find((d) => d.device_id === deviceId);
    void unlock(deviceId, device?.online === true, outcome.ticket);
    await tick();
    buttonOf(deviceId)?.focus();
    return { ok: true } as const;
  }

  const STATUS: Record<UnlockState, MessageKey> = {
    waiting: "hub.floor.remote_unlock_waiting",
    done: "hub.floor.remote_unlock_done",
    failed: "hub.floor.remote_unlock_failed",
  };

  const snapshot = $derived(data.hub.ok ? data.hub.snapshot : null);
  // No zones exist yet, so every device is unassigned.
  const unassigned = $derived(snapshot?.devices.filter((d) => d.zone === null) ?? []);
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.floor") })}</title>
</svelte:head>

<div class="floor">
  <h1 class="type-hub-numeric">{t("hub.nav.floor")}</h1>

  {#if snapshot && !snapshot.branch}
    <section class="note" data-testid="floor-not-set-up">
      <p class="type-hub-title">{t("hub.floor.not_set_up")}</p>
      <p class="type-hub-body muted">{t("hub.floor.not_set_up_body")}</p>
    </section>
  {:else if snapshot}
    <p class="type-hub-body muted" data-testid="floor-no-zones">{t("hub.floor.no_zones")}</p>

    <section class="unassigned" aria-labelledby="floor-unassigned-title">
      <h2 class="type-hub-title" id="floor-unassigned-title">{t("hub.floor.unassigned")}</h2>
      {#if unassigned.length === 0}
        <p class="type-hub-body muted">{t("hub.floor.no_devices")}</p>
      {:else}
        <ul class="devices">
          {#each unassigned as device (device.device_id)}
            <li class="device" data-device-id={device.device_id}>
              <span class="label type-hub-title">{device.label}</span>
              <StatusBadge state="unassigned" />
              <!-- Presence in words (never colour alone): a live paired session or not. -->
              <span class="presence type-hub-body" data-testid="device-presence"
                >{device.online ? t("hub.floor.online") : t("hub.floor.offline")}</span
              >
              {#if device.class === "paired"}
                <span class="unlock" bind:this={wraps[device.device_id]}>
                  <Button
                    label="hub.floor.remote_unlock"
                    variant="secondary"
                    disabled={unlocks[device.device_id] === "waiting" ||
                      approving === device.device_id}
                    disabledReason={approving === device.device_id
                      ? "hub.floor.remote_unlock_needs_approval"
                      : "hub.floor.remote_unlock_waiting"}
                    onclick={() => void unlock(device.device_id, device.online)}
                  />
                </span>
                {#if needsApproval && approving !== device.device_id}
                  <span class="note type-hub-label" data-testid="unlock-needs-approval"
                    >{t("hub.floor.remote_unlock_needs_approval")}</span
                  >
                {/if}
                <span class="status type-hub-label" role="status" data-testid="unlock-status">
                  {#if unlocks[device.device_id]}
                    {#if unlocks[device.device_id] === "failed"}<span aria-hidden="true">⚠</span
                      >{/if}
                    <span>{t(STATUS[unlocks[device.device_id]])}</span>
                  {/if}
                </span>
                {#if approving === device.device_id}
                  <InlinePin
                    message={approvalPrompt}
                    onsubmit={approve}
                    oncancel={() => (approving = null)}
                    returnFocusTo={buttonOf(device.device_id)}
                  />
                {/if}
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>

<style>
  .floor {
    display: grid;
    gap: var(--space-5);
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .muted,
  .presence,
  .note {
    color: var(--text-muted);
  }

  .status {
    display: inline-flex;
    gap: var(--space-2);
    min-height: 1lh;
    color: var(--text);
  }

  .note {
    display: grid;
    gap: var(--space-2);
    max-width: 60ch;
  }

  .unassigned {
    display: grid;
    gap: var(--space-4);
  }

  .devices {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 220px), 1fr));
    gap: var(--space-4);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* An unassigned device: a 2px dashed state-unassigned outline and no state bar. */
  .device {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-hub-tile-padding);
    border: 2px dashed var(--state-unassigned);
    border-radius: var(--radius-md);
    background-color: var(--floor-tile-background);
    overflow-wrap: anywhere;
  }
</style>
