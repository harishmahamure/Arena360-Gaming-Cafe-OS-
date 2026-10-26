<script lang="ts">
  import {
    AlertBadge,
    Button,
    Card,
    FloorTile,
    InlinePin,
    ReasonPicker,
    GroupChip,
    StatusBadge,
    TabControl,
    TextField,
    TierLock,
    focusFirstInvalid,
    reasonOptions,
    reasonsFor,
    t,
  } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";

  let { theme, title }: { theme: "dark" | "hc" | "light"; title: MessageKey } = $props();

  const id = $props.id();
  const hubThemes = $derived(theme !== "light");

  const VARIANTS = [
    { variant: "primary", name: "gallery.variant.primary", label: "gallery.button.primary" },
    { variant: "secondary", name: "gallery.variant.secondary", label: "gallery.button.secondary" },
    { variant: "warning", name: "gallery.variant.warning", label: "gallery.button.warning" },
  ] as const;

  const FORCED = [
    { state: "hover", name: "gallery.state.hover" },
    { state: "pressed", name: "gallery.state.pressed" },
    { state: "focus", name: "gallery.state.focused" },
  ] as const;

  const STATES = [
    "free",
    "in-session",
    "paused",
    "reserved",
    "order-pending",
    "maintenance",
    "offline",
    "unassigned",
  ] as const;

  const TABS = [
    { id: "daily", label: "gallery.tabs.daily" },
    { id: "monthly", label: "gallery.tabs.monthly" },
    { id: "yearly", label: "gallery.tabs.yearly" },
  ] as const;

  const PANELS: Record<string, MessageKey> = {
    daily: "gallery.tabs.panel_daily",
    monthly: "gallery.tabs.panel_monthly",
    yearly: "gallery.tabs.panel_yearly",
  };

  let alerts = $state(3);
  let activated = $state(0);

  const TILES = [
    { state: "free", time: "", group: "teal" },
    { state: "in-session", time: "01:23:45", group: "orange" },
    { state: "paused", time: "00:42:10", group: "lime" },
    { state: "reserved", time: "", group: "sand" },
    { state: "order-pending", time: "00:10:00", group: "coral" },
    { state: "maintenance", time: "", group: "silver" },
    { state: "offline", time: "", group: "teal" },
    { state: "unassigned", time: "", group: "" },
  ] as const;

  // Demo form: submit validates, then focuses the first invalid field.
  let form: HTMLFormElement | undefined = $state();
  let username = $state("");
  let password = $state("");
  let usernameError: MessageKey | null = $state(null);
  let passwordError: MessageKey | null = $state(null);
  let valid = $state(false);

  // Manager approval: a stubbed check where 2222 is the only PIN that approves anything.
  const DEMO_PIN = "2222";
  const REVOKE = "device.revoke";
  const REASONS = reasonOptions(REVOKE);
  const check = async (pin: string) =>
    pin === DEMO_PIN
      ? ({ ok: true } as const)
      : ({ ok: false, message: "ui.pin.rejected" } as const);

  let sampleApproved = $state(false);
  let sampleKey = $state(0);

  // The reason-requiring flow: device.revoke needs a reason (the generated reasons table says
  // so), so Revoke asks for it first, then the PIN, then the stubbed revoke applies.
  let flow: "idle" | "pin" | "reason" = $state("idle");
  let flowReason: string | null = null;
  let flowStatus: { message: MessageKey; params?: Record<string, string> } | null = $state(null);
  let revokeWrap: HTMLElement | undefined = $state();
  const revokeButton = $derived(revokeWrap?.querySelector("button") ?? null);

  /** The stubbed `device.revoke`: without a reason it is refused, as the Hub refuses it. */
  function stubRevoke(reason: string | null): "REASON_REQUIRED" | "applied" {
    return reason ? "applied" : "REASON_REQUIRED";
  }

  function startRevoke() {
    flowStatus = null;
    flowReason = null;
    flow = reasonsFor(REVOKE) ? "reason" : "pin";
  }

  function chooseReason(reason: string) {
    flowReason = reason;
    flow = "pin";
  }

  async function flowPin(pin: string) {
    const result = await check(pin);
    if (!result.ok) return result;
    if (stubRevoke(flowReason) === "REASON_REQUIRED") {
      flow = "reason";
      return result;
    }
    const label = REASONS.find((r) => r.value === flowReason)?.label ?? "reason.other";
    flow = "idle";
    flowStatus = { message: "gallery.approval.revoked", params: { reason: t(label) } };
    await tick();
    revokeButton?.focus();
    return result;
  }

  function flowCancel() {
    flow = "idle";
    flowReason = null;
    flowStatus = { message: "gallery.approval.cancelled" };
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    usernameError = username.trim() ? null : "gallery.form.required";
    passwordError = password ? null : "gallery.form.required";
    valid = !usernameError && !passwordError;
    await tick();
    focusFirstInvalid(form);
  }
</script>

<section class="theme" data-theme={theme} aria-labelledby="{id}-title">
  <h2 id="{id}-title">{t(title)}</h2>

  <section class="group" aria-labelledby="{id}-buttons">
    <h3 id="{id}-buttons">{t("gallery.section.buttons")}</h3>
    {#each VARIANTS as v (v.variant)}
      <h4>{t(v.name)}</h4>
      <div class="row">
        <div class="cell">
          <span class="caption">{t("gallery.state.rest")}</span>
          <Button variant={v.variant} label={v.label} />
        </div>
        {#each FORCED as f (f.state)}
          <div class="cell">
            <span class="caption">{t(f.name)}</span>
            <Button variant={v.variant} label={v.label} forceState={f.state} />
          </div>
        {/each}
        <div class="cell">
          <span class="caption">{t("gallery.state.disabled")}</span>
          <Button
            variant={v.variant}
            label={v.label}
            disabled
            disabledReason={v.variant === "warning"
              ? "gallery.disabled.no_time"
              : v.variant === "primary"
                ? "gallery.disabled.no_device"
                : "gallery.disabled.closed"}
          />
        </div>
      </div>
    {/each}
  </section>

  <section class="group" aria-labelledby="{id}-fields">
    <h3 id="{id}-fields">{t("gallery.section.fields")}</h3>
    <div class="row fields">
      <div class="cell">
        <span class="caption">{t("gallery.state.rest")}</span>
        <TextField label="gallery.field.name" />
      </div>
      <div class="cell">
        <span class="caption">{t("gallery.state.hover")}</span>
        <TextField label="gallery.field.name" forceState="hover" />
      </div>
      <div class="cell">
        <span class="caption">{t("gallery.state.focused")}</span>
        <TextField label="gallery.field.name" forceState="focus" />
      </div>
      <div class="cell">
        <span class="caption">{t("gallery.state.error")}</span>
        <TextField
          label="gallery.field.phone"
          type="tel"
          value="98450"
          error="gallery.field.phone_invalid"
        />
      </div>
      <div class="cell">
        <span class="caption">{t("gallery.field.password")}</span>
        <TextField label="gallery.field.password" type="password" autocomplete="off" />
      </div>
    </div>
  </section>

  <section class="group" aria-labelledby="{id}-badges">
    <h3 id="{id}-badges">{t("gallery.section.badges")}</h3>
    <h4>{t("gallery.section.status")}</h4>
    {#if hubThemes}
      <div class="row">
        {#each STATES as state (state)}
          <StatusBadge {state} />
        {/each}
        {#each ["teal", "orange", "lime", "sand", "coral", "silver"] as colour (colour)}
          <GroupChip name={colour} {colour} />
        {/each}
      </div>
    {:else}
      <p class="note">{t("gallery.hub_only")}</p>
    {/if}
    <h4>{t("gallery.section.alerts")}</h4>
    <div class="row">
      <AlertBadge count={alerts} />
      <AlertBadge tag="new" />
      <AlertBadge tag="call" />
      <AlertBadge tag="time-up" />
      <AlertBadge tag="request" />
      <Button variant="secondary" label="gallery.alert.add" onclick={() => alerts++} />
    </div>
    <h4>{t("gallery.section.floor_tile")}</h4>
    {#if hubThemes}
      <div class="tiles">
        {#each TILES as tile, i (tile.state)}
          <FloorTile
            name="PC {i + 1}"
            state={tile.state}
            time={tile.time || undefined}
            secondary={t("gallery.tile.secondary")}
            group={tile.group ? { name: tile.group, colour: tile.group } : undefined}
            alerts={i === 1 ? 2 : undefined}
            onactivate={() => activated++}
          />
        {/each}
      </div>
      <p class="note" role="status" data-testid="tile-activated">{activated}</p>
    {:else}
      <p class="note">{t("gallery.hub_only")}</p>
    {/if}
    <h4>{t("gallery.section.tier_lock")}</h4>
    <div class="row">
      <TierLock tier="Basic" />
      <TierLock tier="Premium" billing />
    </div>
  </section>

  <section class="group" aria-labelledby="{id}-cards">
    <h3 id="{id}-cards">{t("gallery.section.cards")}</h3>
    <div class="row cards">
      <Card>
        <h4>{t("gallery.card.title")}</h4>
        <p>{t("gallery.card.body")}</p>
        <a class="link" href="#{id}-form">{t("gallery.card.link")}</a>
      </Card>
      <Card context="web">
        <h4>{t("gallery.card.web_title")}</h4>
        <p>{t("gallery.card.web_body")}</p>
      </Card>
    </div>
  </section>

  <section class="group" aria-labelledby="{id}-tabs">
    <h3 id="{id}-tabs">{t("gallery.section.tabs")}</h3>
    <div class="row">
      <div class="cell">
        <span class="caption">{t("gallery.state.rest")}</span>
        <TabControl items={[...TABS]} label="gallery.tabs.label" selected="monthly">
          {#snippet panel(selected)}{t(PANELS[selected])}{/snippet}
        </TabControl>
      </div>
      <div class="cell">
        <span class="caption">{t("gallery.state.focused")}</span>
        <TabControl items={[...TABS]} label="gallery.tabs.label" forceState="focus" />
      </div>
    </div>
  </section>

  <section class="group" aria-labelledby="{id}-form">
    <h3 id="{id}-form">{t("gallery.section.form")}</h3>
    <form
      class="demo-form"
      bind:this={form}
      onsubmit={submit}
      oninput={() => (valid = false)}
      novalidate
    >
      <TextField
        label="gallery.field.username"
        name="username"
        autocomplete="username"
        required
        bind:value={username}
        error={usernameError}
      />
      <TextField
        label="gallery.field.password"
        name="password"
        type="password"
        autocomplete="current-password"
        required
        bind:value={password}
        error={passwordError}
      />
      <Button type="submit" label="gallery.form.submit" />
      <p class="status" role="status">{valid ? t("gallery.form.valid") : ""}</p>
    </form>
  </section>

  <section class="group" aria-labelledby="{id}-approval">
    <h3 id="{id}-approval">{t("gallery.section.approval")}</h3>
    <p class="note">{t("gallery.approval.hint")}</p>
    <div class="row">
      <div class="cell" data-testid="pin-sample">
        <span class="caption">{t("gallery.approval.pin")}</span>
        {#key sampleKey}
          <InlinePin
            autofocus={false}
            onsubmit={async (pin) => {
              const result = await check(pin);
              sampleApproved = result.ok;
              return result;
            }}
            oncancel={() => {
              sampleApproved = false;
              sampleKey++;
            }}
          />
        {/key}
        <p class="status" role="status">{sampleApproved ? t("gallery.approval.approved") : ""}</p>
      </div>
      <div class="cell">
        <span class="caption">{t("gallery.approval.reasons")}</span>
        <ReasonPicker
          autofocus={false}
          options={REASONS}
          onconfirm={() => Promise.reject(new Error("offline"))}
          oncancel={() => {}}
        />
      </div>
      <div class="cell" data-testid="reason-flow">
        <span class="caption">{t("gallery.approval.flow")}</span>
        <span bind:this={revokeWrap}>
          <Button variant="warning" label="gallery.approval.revoke" onclick={startRevoke} />
        </span>
        {#if flow === "pin"}
          <InlinePin onsubmit={flowPin} oncancel={flowCancel} returnFocusTo={revokeButton} />
        {:else if flow === "reason"}
          <ReasonPicker
            options={REASONS}
            onconfirm={chooseReason}
            oncancel={flowCancel}
            returnFocusTo={revokeButton}
          />
        {/if}
        <p class="status" role="status">
          {flowStatus ? t(flowStatus.message, flowStatus.params) : ""}
        </p>
      </div>
    </div>
  </section>
</section>

<style>
  .theme {
    padding: var(--space-6) var(--space-web-gutter-phone);
    border-top: 1px solid var(--line);
  }

  h2 {
    margin: 0 0 var(--space-5);
    font-size: var(--type-hub-numeric-size);
    font-weight: var(--type-hub-title-weight);
  }

  h3 {
    margin: 0 0 var(--space-4);
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  h4 {
    margin: var(--space-4) 0 var(--space-3);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    color: var(--text-muted);
  }

  .group {
    margin-bottom: var(--space-7);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-5);
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 240px), 1fr));
    gap: var(--space-4);
  }

  .cell {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    max-width: 100%;
  }

  .fields .cell {
    flex: 1 1 16rem;
  }

  .cards > :global(*) {
    flex: 1 1 18rem;
  }

  .caption,
  .note {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
  }

  .demo-form {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-4);
    max-width: 28rem;
  }

  .demo-form > :global([data-component="text-field"]) {
    align-self: stretch;
  }

  .status {
    margin: 0;
    color: var(--ok);
  }
</style>
