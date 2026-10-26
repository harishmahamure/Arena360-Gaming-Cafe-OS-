<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    hasPermission,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { sessionStaff, snapshotOf } from "../../../lib/operating";

  const STATES: Record<string, MessageKey> = {
    running: "hub.settings.setup.test_session.running",
    passed: "hub.settings.setup.test_session.passed",
    failed: "hub.settings.setup.test_session.failed",
  };

  const snapshot = $derived(snapshotOf(page.data));
  const setup = $derived(snapshot?.setup);
  const groups = $derived(snapshot?.groups.length ?? 0);
  const canManage = $derived(hasPermission(sessionStaff(page.data), "device.manage"));
  const kiosks = $derived(
    snapshot?.devices.filter(
      (d) =>
        d.class === "paired" &&
        d.kind === "pc" &&
        d.group_id &&
        d.online &&
        d.status !== "maintenance",
    ) ?? [],
  );
  let picked = $state("");
  let busy = $state(false);
  let refusal: string | null = $state(null);
  const chosen = $derived(
    kiosks.some((d) => d.device_id === picked) ? picked : (kiosks[0]?.device_id ?? ""),
  );

  async function start() {
    if (busy || !chosen) return;
    busy = true;
    refusal = null;
    const outcome = await submitIntent(
      { kind: "device.test_session.start", device_id: chosen },
      newIdempotencyKey(),
    );
    busy = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "error" && outcome.error.code === "VALIDATION_FAILED") {
      refusal = outcome.error.message;
    } else if (outcome.kind !== "applied") {
      refusal = t("hub.settings.setup.test_session.error");
    }
  }
  const rows: { label: MessageKey; done: boolean; href?: string }[] = $derived([
    {
      label: "hub.settings.setup.pair",
      done: !!snapshot?.devices.some((d) => d.class === "paired"),
      href: "/settings/devices",
    },
    { label: "hub.settings.setup.groups", done: groups > 0, href: "/settings/layout" },
    {
      label: "hub.settings.setup.rates",
      done: groups > 0 && setup?.groups_without_rate === 0,
      href: "/settings/layout",
    },
    {
      label: "hub.settings.setup.printer",
      done: setup?.printer === "ok" || setup?.printer === "skipped",
      href: "/settings/printer",
    },
    { label: "hub.settings.setup.test_session", done: setup?.test_session === "passed" },
  ]);
</script>

<section aria-labelledby="setup-title">
  <h2 id="setup-title" class="type-hub-title">{t("hub.settings.setup.title")}</h2>
  <p class="intro">{t("hub.settings.setup.intro")}</p>
  <ul data-testid="setup-list">
    {#each rows as r (r.label)}
      <li class="row" data-done={r.done}>
        <span class="name">{t(r.label)}</span>
        <span class="state"
          >{t(r.done ? "hub.settings.setup.done" : "hub.settings.setup.todo")}</span
        >
        {#if r.href}
          <a class="link ui-interactive" href={r.href}
            >{t("hub.settings.setup.open", { screen: t(r.label) })}</a
          >
        {/if}
        {#if r.label === "hub.settings.setup.test_session"}
          {#if setup && STATES[setup.test_session]}
            <p class="note" role="status" data-testid="test-session-state">
              {t(STATES[setup.test_session])}
            </p>
          {/if}
          {#if refusal}
            <p class="note error" role="alert" data-testid="test-session-refusal">
              <span aria-hidden="true">⚠</span>
              {refusal}
            </p>
          {/if}
          {#if canManage && setup?.test_session !== "running"}
            {#if kiosks.length}
              <div class="start">
                <label for="test-session-pick">{t("hub.settings.setup.test_session.pick")}</label>
                <select
                  id="test-session-pick"
                  class="ui-interactive select"
                  value={chosen}
                  onchange={(e) => (picked = e.currentTarget.value)}
                >
                  {#each kiosks as d (d.device_id)}<option value={d.device_id}>{d.label}</option
                    >{/each}
                </select>
                <Button
                  label="hub.settings.setup.test_session.start"
                  disabled={busy}
                  disabledReason="hub.settings.saving"
                  onclick={start}
                />
              </div>
            {:else}
              <p class="note" data-testid="test-session-none">
                {t("hub.settings.setup.test_session.none")}
              </p>
            {/if}
          {/if}
        {/if}
      </li>
    {/each}
  </ul>
</section>

<style>
  h2,
  .intro {
    margin: 0 0 var(--space-3);
  }

  .intro {
    color: var(--text-muted);
  }

  ul {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
  }

  .name {
    flex: 1;
    font-weight: var(--type-hub-title-weight);
  }

  .state {
    font-weight: var(--type-hub-title-weight);
  }

  .note {
    flex-basis: 100%;
    margin: 0;
  }

  .error {
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }

  .start {
    display: flex;
    flex-basis: 100%;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .select {
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

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--text);
  }
</style>
