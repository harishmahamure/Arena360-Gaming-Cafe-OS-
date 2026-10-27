<script lang="ts">
  import {
    BillBreakdown,
    Button,
    InlinePin,
    ReasonPicker,
    StatusBadge,
    TextField,
    formatDateTime,
    formatMoney,
    t,
  } from "@arena/ui";
  import type {
    SessionStaff,
    SnapshotBill,
    SnapshotDevice,
    SnapshotGroup,
    SnapshotLocale,
    SnapshotSession,
    PauseReason,
    PaymentRow,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { onMount, tick } from "svelte";
  import {
    SESSION_DEPENDENCY,
    dropElevation,
    elevate,
    hasPermission,
    newIdempotencyKey,
  } from "./auth";
  import ConvertGuest from "./players/ConvertGuest.svelte";
  import ExtendOptions from "./players/ExtendOptions.svelte";
  import PlayerLookup from "./players/PlayerLookup.svelte";
  import StartOptions from "./players/StartOptions.svelte";
  import SettleBar from "./SettleBar.svelte";
  import { SNAPSHOT_DEPENDENCY } from "./snapshot";
  import {
    MESSAGE_MAX,
    PAUSE_LABELS,
    adminUnlockJob,
    assignJob,
    cmds,
    endJob,
    extendJob,
    maintenanceJob,
    pauseJob,
    permissionFor,
    powerJob,
    resumeJob,
    screenJob,
    send,
    settleJob,
    startJob,
    type ExtendChoice,
    type Job,
    type StartChoice,
  } from "./tile-actions.svelte";

  type Badge = {
    state: "free" | "offline" | "in-session" | "paused" | "reserved" | "maintenance";
    label: MessageKey;
  };

  const PAUSE_OPTIONS = (Object.keys(PAUSE_LABELS) as PauseReason[]).map((value) => ({
    value,
    label: PAUSE_LABELS[value],
  }));

  let {
    device,
    group,
    groups,
    zone,
    rate,
    badge,
    staff,
    tz,
    session,
    bill,
    unpaid,
    locale,
    names,
    pauseText,
    onclose,
  }: {
    device: SnapshotDevice;
    group: SnapshotGroup | undefined;
    groups: SnapshotGroup[];
    zone: string | undefined;
    rate: string | null;
    badge: Badge;
    staff: SessionStaff | null;
    tz: string | undefined;
    session: SnapshotSession | undefined;
    bill: SnapshotBill | undefined;
    unpaid: SnapshotBill | undefined;
    locale: SnapshotLocale | null;
    names: Record<string, string>;
    pauseText: (session: SnapshotSession) => string;
    onclose: () => void;
  } = $props();

  let root: HTMLElement | undefined = $state();
  let settleBar: SettleBar | undefined = $state();
  let heading: HTMLElement | undefined = $state();
  let text = $state("");
  let textError: MessageKey | null = $state(null);
  let pick = $state("");
  let picked: { id?: string; name?: string } | null = $state(null);
  let choice: StartChoice = $state({ mode: "postpaid" });
  let pausing = $state(false);
  let extending = $state(false);
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");

  const id = $props.id();
  const titleId = id + "-title";
  const cmd = $derived(cmds[device.device_id]);
  const busy = $derived(cmd?.state === "waiting" || approving !== null);
  const isPc = $derived(device.kind === "pc");
  const assigned = $derived(group !== undefined);
  const maintenance = $derived(device.status === "maintenance");
  const offline = $derived(device.status === "offline");
  const live = $derived(isPc && assigned && !maintenance && !offline);
  const fitting = $derived(groups.filter((g) => g.kind === "pc"));
  const needsRemote = $derived(!hasPermission(staff, "device.remote"));
  const free = $derived(device.status === "online");

  $effect(() => {
    if (!free) picked = null;
  });
  $effect(() => {
    void picked?.id;
    choice = { mode: "postpaid" };
  });
  const START_LABEL: Record<StartChoice["mode"], MessageKey> = {
    postpaid: "hub.session.start",
    pack: "hub.session.start_pack",
    pass: "hub.session.start_pass",
    block: "hub.session.start_block",
    wallet: "hub.session.start_wallet",
  };
  const billing = $derived(session !== undefined);
  $effect(() => {
    if (!billing) return;
    const timer = setInterval(() => void invalidate(SNAPSHOT_DEPENDENCY), 10_000);
    return () => clearInterval(timer);
  });
  const canLookup = $derived(hasPermission(staff, "player.view"));
  const unassignedRefusal = $derived(
    cmd?.state === "refused" && cmd.refusal === "hub.session.unassigned",
  );

  onMount(() => {
    heading?.focus();
    return () => (approving = null);
  });

  async function go(job: Job, ticket?: string) {
    if (!ticket && !hasPermission(staff, permissionFor(job.payload.kind))) {
      approvalPrompt = "ui.pin.prompt";
      approving = job;
      return;
    }
    const result = await send(device.device_id, job, device.online, ticket);
    if (result === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (result === "pin_required") {
      approvalPrompt = "ui.pin.again";
      approving = job;
      return;
    }
    if (job.track === "none") await invalidate(SNAPSHOT_DEPENDENCY);
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
    if (outcome.kind === "unreachable")
      return { ok: false, message: "ui.pin.unreachable" } as const;
    if (outcome.kind === "error") return { ok: false, message: "ui.pin.failed" } as const;
    if (approving !== job) {
      void dropElevation(outcome.ticket);
      return { ok: true } as const;
    }
    approving = null;
    void go(job, outcome.ticket);
    await tick();
    heading?.focus();
    return { ok: true } as const;
  }

  function sendMessage(event: SubmitEvent) {
    event.preventDefault();
    const body = text.trim();
    if (body.length < 1 || [...body].length > MESSAGE_MAX) {
      textError = "hub.sheet.message_invalid";
      return;
    }
    textError = null;
    void go(screenJob(device.device_id, "message", body));
  }

  async function focusAssign() {
    await tick();
    root?.querySelector<HTMLElement>('[data-testid="sheet-assign"] select')?.focus();
  }

  function assign(event: SubmitEvent) {
    event.preventDefault();
    if (pick) void go(assignJob(device.device_id, pick, device.meta_rev ?? undefined));
  }

  async function cancelPause() {
    pausing = false;
    await tick();
    root?.querySelector<HTMLElement>('[data-testid="session-bill"] button')?.focus();
  }

  async function choosePause(value: string, note?: string) {
    if (!session) return;
    pausing = false;
    await go(pauseJob(session.id, session.rev, value as PauseReason, note));
  }

  const canExtend = $derived(
    session?.player_id != null &&
      group !== undefined &&
      (session.mode === "pack" || session.mode === "wallet") &&
      ["active", "grace"].includes(session.state),
  );

  $effect(() => {
    if (!canExtend) extending = false;
  });
  $effect(() => {
    if (cmd?.state === "done" && cmd.job.payload.kind === "session.extend") extending = false;
  });

  function extend(choice: ExtendChoice) {
    if (session) void go(extendJob(session.id, choice));
  }

  const creditRefused = $derived(
    cmd?.state === "refused" &&
      cmd.refusal === "hub.settle.credit_limit" &&
      (cmd.job.payload.kind === "session.settle" || cmd.job.payload.kind === "session.extend")
      ? cmd.job
      : null,
  );

  function overrideCredit() {
    const refused = creditRefused;
    if (
      !refused ||
      (refused.payload.kind !== "session.settle" && refused.payload.kind !== "session.extend")
    ) {
      return;
    }
    const job: Job = {
      ...refused,
      key: newIdempotencyKey(),
      payload: { ...refused.payload, credit_override: true },
    };
    if (hasPermission(staff, "credit.manage")) return void go(job);
    approvalPrompt = "ui.pin.prompt";
    approving = job;
  }

  function settle(payments: PaymentRow[]) {
    if (unpaid?.order_id && unpaid.order_rev != null) {
      void go(settleJob(unpaid.order_id, payments, unpaid.order_rev));
    }
  }

  let convertedName: string | null = $state(null);

  async function converted(name: string) {
    convertedName = name;
    await invalidate(SNAPSHOT_DEPENDENCY);
  }

  const settled = $derived(
    cmd?.state === "done" && cmd.job.payload.kind === "session.settle" ? cmd.job.payload : null,
  );

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && settleBar) {
      event.preventDefault();
      void settleBar.submit();
      return;
    }
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    onclose();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  class="sheet"
  data-component="tile-sheet"
  data-testid="tile-sheet"
  aria-labelledby={titleId}
  bind:this={root}
  {onkeydown}
>
  <div class="head">
    <h2 class="type-hub-title" id={titleId} tabindex="-1" bind:this={heading}>{device.label}</h2>
    <Button label="hub.sheet.close" variant="secondary" onclick={onclose} />
  </div>

  <dl class="details type-hub-label">
    <div>
      <dt>{t("hub.sheet.group")}</dt>
      <dd data-testid="sheet-group">{group ? group.name : t("hub.sheet.no_group")}</dd>
    </div>
    <div>
      <dt>{t("hub.sheet.zone")}</dt>
      <dd data-testid="sheet-zone">{zone ?? t("hub.sheet.no_zone")}</dd>
    </div>
    <div>
      <dt>{t("hub.sheet.rate")}</dt>
      <dd data-testid="sheet-rate">
        {rate ? t("hub.sheet.rate_value", { rate }) : t("hub.sheet.no_rate")}
      </dd>
    </div>
    <div>
      <dt>{t("hub.sheet.status")}</dt>
      <dd data-testid="sheet-status">
        <StatusBadge
          state={assigned ? badge.state : "unassigned"}
          label={assigned ? badge.label : undefined}
        />
      </dd>
    </div>
    {#if session?.state === "paused"}
      <div>
        <dt>{t("hub.pause.paused_for")}</dt>
        <dd data-testid="sheet-pause">{pauseText(session)}</dd>
      </div>
    {/if}
    {#if device.tv_number}
      <div>
        <dt>{t("hub.floor.tv", { n: device.tv_number })}</dt>
        <dd></dd>
      </div>
    {/if}
    {#if maintenance && device.maintenance_note}
      <div>
        <dt>{t("hub.floor.unavailable")}</dt>
        <dd>{device.maintenance_note}</dd>
      </div>
    {/if}
    {#if offline && device.last_seen_at}
      <div>
        <dt>{t("hub.floor.last_seen", { at: formatDateTime(device.last_seen_at, tz) })}</dt>
        <dd></dd>
      </div>
    {/if}
  </dl>

  {#if convertedName}
    <p class="type-hub-label" role="status" data-testid="convert-done">
      {t("hub.session.converted", { name: convertedName })}
    </p>
  {/if}

  <p class="result type-hub-label" role="status" data-testid="unlock-status">
    {#if cmd}
      {#if cmd.state === "failed"}<span aria-hidden="true">⚠</span>
        <span>{t("hub.floor.remote_unlock_failed")}</span>
      {:else if cmd.state === "refused"}<span aria-hidden="true">⚠</span>
        <span>{cmd.detail ?? t(cmd.refusal ?? "hub.settings.failed")}</span>
      {:else}<span>{t(cmd.state === "waiting" ? cmd.job.waiting : cmd.job.done)}</span>{/if}
    {/if}
  </p>
  {#if creditRefused}
    <div>
      <Button
        label="hub.settle.credit_override"
        variant="secondary"
        disabled={busy}
        disabledReason="hub.sheet.waiting"
        onclick={overrideCredit}
      />
    </div>
  {/if}
  {#if unassignedRefusal}
    <div>
      <Button label="hub.sheet.assign" variant="secondary" onclick={() => void focusAssign()} />
    </div>
  {/if}
  {#if cmd?.state === "failed"}
    <div>
      <Button
        label="hub.sheet.retry"
        variant="secondary"
        onclick={() =>
          void go(cmd.job.track === "exit" ? adminUnlockJob(device.device_id) : cmd.job)}
      />
    </div>
  {/if}

  {#if approving}
    <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
  {/if}

  {#if unpaid && locale}
    <SettleBar bind:this={settleBar} bill={unpaid} {locale} {busy} onsettle={settle}>
      {#snippet extra()}
        {#if unpaid.guest && unpaid.session_rev != null}
          <ConvertGuest
            sessionId={unpaid.session_id}
            rev={unpaid.session_rev}
            {staff}
            {busy}
            ondone={converted}
          />
        {/if}
      {/snippet}
    </SettleBar>
  {:else if settled && locale}
    <p class="type-hub-numeric" role="status" data-testid="settle-done">
      {t("hub.settle.settled", {
        amount: formatMoney(
          settled.payments.reduce((sum, p) => sum + p.amount_minor, 0),
          locale,
        ),
      })}
    </p>
  {/if}

  {#if !isPc}
    <p class="note type-hub-body" data-testid="sheet-details-only">{t("hub.sheet.details_only")}</p>
  {:else if !assigned}
    <form class="section" onsubmit={assign} data-testid="sheet-assign">
      <label class="field type-hub-label">
        <span>{t("hub.sheet.assign_pick")}</span>
        <select class="ui-interactive select" bind:value={pick}>
          <option value="">{t("hub.sheet.assign_none")}</option>
          {#each fitting as g (g.id)}<option value={g.id}>{g.name}</option>{/each}
        </select>
      </label>
      {#if fitting.length === 0}
        <p class="note type-hub-label">{t("hub.sheet.assign_empty")}</p>
      {/if}
      <Button
        label="hub.sheet.assign"
        type="submit"
        disabled={busy || !pick}
        disabledReason={busy ? "hub.sheet.waiting" : "hub.sheet.assign_none"}
      />
    </form>
  {:else if offline}
    <p class="note type-hub-body" data-testid="sheet-offline">{t("hub.sheet.offline")}</p>
  {:else if maintenance}
    <div class="section">
      <Button
        label="hub.sheet.return_to_service"
        disabled={busy}
        disabledReason="hub.sheet.waiting"
        onclick={() => void go(maintenanceJob(device.device_id, device.meta_rev ?? undefined))}
      />
    </div>
    {@render admin()}
  {:else if live}
    {#if free}
      <div class="section" data-testid="session-start">
        {#if canLookup}
          <PlayerLookup
            onpick={(hit) => (picked = { id: hit.player_id, name: hit.name })}
            onguest={() => (picked = {})}
          />
        {:else}
          <Button
            label="hub.players.lookup.guest"
            variant="secondary"
            onclick={() => (picked = {})}
          />
        {/if}
        <p class="type-hub-label" role="status" data-testid="start-picked">
          {#if picked?.id}{t("hub.session.picked", { name: picked.name ?? "" })}
          {:else if picked}{t("hub.players.lookup.guest_picked")}{/if}
        </p>
        <p class="type-hub-label" data-testid="start-rate">
          {rate ? t("hub.session.rate", { rate }) : t("hub.sheet.no_rate")}
        </p>
        {#if picked && group}
          <StartOptions
            playerId={picked.id ?? null}
            groupId={group.id}
            {locale}
            {rate}
            disabled={busy}
            bind:value={choice}
          />
        {/if}
        <Button
          label={START_LABEL[choice.mode]}
          disabled={busy || !picked}
          disabledReason={busy ? "hub.sheet.waiting" : "hub.session.pick_first"}
          onclick={() =>
            picked && void go(startJob(device.device_id, picked.id, device.manual, choice))}
        />
      </div>
    {:else if session}
      <div class="section" data-testid="session-bill">
        {#if bill && locale}
          <BillBreakdown
            lines={bill.lines}
            subtotal={bill.subtotal_minor}
            paid={bill.paid_minor}
            remaining={bill.remaining_minor}
            {locale}
            {tz}
            {names}
          />
        {/if}
        {#if session.state === "paused"}
          <Button
            label="hub.session.resume"
            disabled={busy}
            disabledReason="hub.sheet.waiting"
            onclick={() => void go(resumeJob(session.id, session.rev))}
          />
        {:else if pausing}
          <ReasonPicker
            options={PAUSE_OPTIONS}
            legend="hub.pause.legend"
            noteFor="other"
            noteLabel="hub.pause.note"
            onconfirm={choosePause}
            oncancel={cancelPause}
          />
        {:else}
          <Button
            label="hub.session.pause"
            variant="secondary"
            disabled={busy}
            disabledReason="hub.sheet.waiting"
            onclick={() => (pausing = true)}
          />
        {/if}
        {#if canExtend && extending && session.player_id && group}
          <ExtendOptions
            playerId={session.player_id}
            groupId={group.id}
            ownPackId={session.pack_id ?? null}
            rateMinor={group.rate_minor ?? null}
            {locale}
            disabled={busy}
            onconfirm={extend}
            oncancel={() => (extending = false)}
          />
        {:else if canExtend}
          <Button
            label="hub.session.extend"
            variant="secondary"
            disabled={busy}
            disabledReason="hub.sheet.waiting"
            onclick={() => (extending = true)}
          />
        {/if}
        <Button
          label="hub.session.end"
          disabled={busy}
          disabledReason="hub.sheet.waiting"
          onclick={() => void go(endJob(session.id, session.rev))}
        />
        {#if !session.player_id}
          <ConvertGuest
            sessionId={session.id}
            rev={session.rev}
            {staff}
            {busy}
            ondone={converted}
          />
        {/if}
      </div>
    {/if}
    <div class="section" role="group" aria-label={t("hub.sheet.screen")}>
      <Button
        label="hub.sheet.unlock"
        disabled={busy}
        disabledReason="hub.sheet.waiting"
        onclick={() => void go(screenJob(device.device_id, "unlock"))}
      />
      <form class="compose" onsubmit={sendMessage}>
        <TextField label="hub.sheet.message_label" bind:value={text} error={textError} />
        <span class="count type-hub-label tabular-nums"
          >{t("hub.sheet.message_count", { count: [...text].length, max: MESSAGE_MAX })}</span
        >
        <Button
          label="hub.sheet.message"
          type="submit"
          disabled={busy}
          disabledReason="hub.sheet.waiting"
        />
      </form>
    </div>
    <div class="section">
      <Button
        label="hub.sheet.lock"
        variant="secondary"
        disabled={busy}
        disabledReason="hub.sheet.waiting"
        onclick={() => void go(screenJob(device.device_id, "lock"))}
      />
    </div>
    <div class="section" role="group" aria-label={t("hub.sheet.power")}>
      <Button
        label="hub.sheet.restart"
        variant="secondary"
        disabled={busy}
        disabledReason="hub.sheet.waiting"
        onclick={() => void go(powerJob(device.device_id, "restart"))}
      />
      <Button
        label="hub.sheet.shutdown"
        variant="secondary"
        disabled={busy}
        disabledReason="hub.sheet.waiting"
        onclick={() => void go(powerJob(device.device_id, "shutdown"))}
      />
      {#if needsRemote}
        <span class="note type-hub-label" data-testid="power-needs-approval"
          >{t("hub.sheet.power_needs_approval")}</span
        >
      {/if}
    </div>
    {@render admin()}
  {/if}
</aside>

{#snippet admin()}
  <div class="section">
    <Button
      label="hub.sheet.admin_unlock"
      variant="secondary"
      disabled={busy}
      disabledReason="hub.sheet.waiting"
      onclick={() => void go(adminUnlockJob(device.device_id))}
    />
    {#if needsRemote}
      <span class="note type-hub-label" data-testid="unlock-needs-approval"
        >{t("hub.floor.remote_unlock_needs_approval")}</span
      >
    {/if}
  </div>
{/snippet}

<style>
  .sheet {
    display: grid;
    gap: var(--space-5);
    box-sizing: border-box;
    min-width: 0;
    padding: var(--space-hub-panel-padding);
    border: var(--tile-sheet-border);
    border-radius: var(--radius-xl);
    background-color: var(--tile-sheet-background);
    color: var(--text);
  }

  :global([data-theme="hc"]) .sheet {
    border: var(--tile-sheet-border-hc);
    background-color: var(--tile-sheet-background-hc);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  h2,
  p,
  dl,
  dd {
    margin: 0;
  }

  h2:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .details {
    display: grid;
    gap: var(--space-2);
  }

  .details > div {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  dt {
    color: var(--text-muted);
  }

  .result {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .section {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .compose {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
    width: 100%;
  }

  .field {
    display: grid;
    gap: var(--space-2);
  }

  .note {
    color: var(--text-muted);
  }
</style>
