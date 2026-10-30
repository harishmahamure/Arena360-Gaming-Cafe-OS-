<script lang="ts">
  import { Button, GroupChip, StatusBadge, t } from "@arena/ui";
  import { cmd } from "./commands.svelte";
  import { colourOf, deviceKindText, deviceLines, groupEditable, type DeviceLine } from "./devices";
  import GroupForm from "./GroupForm.svelte";
  import MoveForm from "./MoveForm.svelte";
  import { listed, sourceOf, str, fieldsOf, type BranchData, type Listed } from "./pricing";
  import PricingRow from "./PricingRow.svelte";
  import RevokeForm from "./RevokeForm.svelte";

  let { data, scope }: { data: BranchData[]; scope: string } = $props();

  type Mode =
    | { kind: "group"; item: Listed | null }
    | { kind: "move"; line: DeviceLine }
    | { kind: "revoke"; line: DeviceLine }
    | null;
  let mode: Mode = $state(null);

  const groups = $derived(listed(data, (c) => c.groups));
  const lines = $derived(deviceLines(data));
  const many = $derived(data.length > 1);

  // The form closes once its change was sent or queued.
  let seen = cmd.sent;
  $effect(() => {
    if (cmd.sent !== seen) {
      seen = cmd.sent;
      mode = null;
    }
  });
</script>

<section class="panel" aria-labelledby="dv-groups">
  <h2 id="dv-groups" class="type-hub-title">{t("owner.devices.groups")}</h2>
  {#if !groups.length}
    <p>{t("owner.devices.groups.empty")}</p>
  {:else}
    <ul class="rows">
      {#each groups as g (g.key)}
        {@const source = sourceOf(g)}
        <PricingRow
          kind="group"
          name={g.name}
          edit={source && groupEditable(source.row)
            ? {
                label: t("owner.devices.group.edit", { name: g.name }),
                onclick: () => (mode = { kind: "group", item: g }),
              }
            : undefined}
        >
          <GroupChip name={g.name} colour={source ? colourOf(source.row) : "sand"} />
          {#if source && str(fieldsOf(source.row).group_kind)}
            <span class="muted type-hub-micro"
              >{deviceKindText(str(fieldsOf(source.row).group_kind) ?? "")}</span
            >
          {/if}
        </PricingRow>
      {/each}
    </ul>
  {/if}
</section>

<section class="panel" aria-labelledby="dv-devices">
  <h2 id="dv-devices" class="type-hub-title">{t("owner.devices.title")}</h2>
  {#if !lines.length}
    <p>{t("owner.devices.empty")}</p>
  {:else}
    <ul class="rows">
      {#each lines as line (line.data.branch.id + line.device.id)}
        <PricingRow
          kind="device"
          name={line.device.name}
          edit={line.device.revoked
            ? undefined
            : {
                label: t("owner.devices.move", { name: line.device.name }),
                onclick: () => (mode = { kind: "move", line }),
              }}
          more={line.device.revoked
            ? undefined
            : {
                label: t("owner.devices.revoke", { name: line.device.name }),
                onclick: () => (mode = { kind: "revoke", line }),
              }}
        >
          <span class="type-hub-label">{line.device.name}</span>
          <span class="muted type-hub-micro">
            {[many ? line.data.branch.name : "", deviceKindText(line.device.kind)]
              .filter(Boolean)
              .join(" · ")}
          </span>
          <span class="type-hub-micro">
            {#if line.group}
              <GroupChip name={line.group.name} colour={colourOf(line.group)} />
            {:else}
              <span class="muted">{t("owner.devices.unassigned")}</span>
            {/if}
            {#if line.device.revoked}
              <StatusBadge tone="danger" text={t("owner.devices.revoked")} />
            {/if}
          </span>
        </PricingRow>
      {/each}
    </ul>
  {/if}
</section>

{#if mode?.kind === "group"}
  {#key mode.item?.key ?? ""}
    <GroupForm item={mode.item} {data} {scope} oncancel={() => (mode = null)} />
  {/key}
{:else if mode?.kind === "move"}
  <MoveForm line={mode.line} oncancel={() => (mode = null)} />
{:else if mode?.kind === "revoke"}
  <RevokeForm line={mode.line} oncancel={() => (mode = null)} />
{:else}
  <div class="add">
    <Button
      label="owner.devices.group.add"
      variant="secondary"
      onclick={() => (mode = { kind: "group", item: null })}
    />
  </div>
{/if}

<style>
  .panel {
    display: grid;
    gap: var(--space-3);
  }
  h2,
  p {
    margin: 0;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--line);
  }
  .muted {
    color: var(--text-muted);
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
