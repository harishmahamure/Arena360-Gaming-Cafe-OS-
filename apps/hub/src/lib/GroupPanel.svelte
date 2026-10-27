<script lang="ts" module>
  export type Payer = { player_id?: string; guest_no?: number };
</script>

<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { PlayerHit, SnapshotGroupBill, SnapshotSession } from "@arena/ui/generated";
  import PlayerLookup from "./players/PlayerLookup.svelte";

  let {
    group,
    members,
    names,
    canLookup,
    busy,
    onpayer,
  }: {
    group: SnapshotGroupBill;
    members: SnapshotSession[];
    names: Record<string, string>;
    canLookup: boolean;
    busy: boolean;
    onpayer: (payer: Payer) => void;
  } = $props();

  let changing = $state(false);
  let pick = $state("");

  const payer = $derived(
    group.payer_username ??
      (group.payer_guest_no != null ? t("hub.session.guest", { n: group.payer_guest_no }) : ""),
  );
  const label = (m: SnapshotSession) =>
    t("hub.group.payer_member", {
      name: m.username ?? t("hub.session.guest", { n: m.guest_no ?? 0 }),
      device: names[m.device_id] ?? "",
    });
  const options = $derived(
    members.flatMap((m) => {
      const value = m.player_id ? `p:${m.player_id}` : m.guest_no != null ? `g:${m.guest_no}` : ""; // i18n-ignore
      return value ? [{ value, text: label(m) }] : [];
    }),
  );

  function save(event: SubmitEvent) {
    event.preventDefault();
    if (!pick) return;
    const [kind, value] = [pick.slice(0, 1), pick.slice(2)];
    changing = false;
    onpayer(kind === "p" ? { player_id: value } : { guest_no: Number(value) });
  }

  function looked(hit: PlayerHit) {
    changing = false;
    onpayer({ player_id: hit.player_id });
  }
</script>

<section class="group" data-testid="group-panel">
  <p class="type-hub-label" data-testid="group-payer">{t("hub.group.payer_line", { payer })}</p>
  {#if changing}
    <form class="change" onsubmit={save} data-testid="group-payer-form">
      {#if options.length > 0}
        <label class="field type-hub-label">
          <span>{t("hub.group.payer_pick")}</span>
          <select class="ui-interactive select" bind:value={pick} data-testid="group-payer-select">
            <option value="">—</option>
            {#each options as o (o.value)}<option value={o.value}>{o.text}</option>{/each}
          </select>
        </label>
        <Button
          label="hub.group.payer_save"
          type="submit"
          disabled={busy || !pick}
          disabledReason={busy ? "hub.sheet.waiting" : "hub.group.payer_pick"}
        />
      {/if}
      {#if canLookup}<PlayerLookup onpick={looked} />{/if}
      <Button
        label="hub.group.payer_cancel"
        variant="secondary"
        onclick={() => (changing = false)}
      />
    </form>
  {:else}
    <Button
      label="hub.group.payer_change"
      variant="secondary"
      disabled={busy}
      disabledReason="hub.sheet.waiting"
      onclick={() => (changing = true)}
    />
  {/if}
</section>

<style>
  .group,
  .change {
    display: grid;
    justify-items: start;
    gap: var(--space-3);
    width: 100%;
  }

  p {
    margin: 0;
  }

  .field {
    display: grid;
    gap: var(--space-2);
  }
</style>
