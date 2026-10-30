<script lang="ts">
  import { Button, StatusBadge, TextField, t } from "@arena/ui";
  import { cmd, propose } from "./commands.svelte";
  import { listed, sourceOf, type BranchData, type Listed } from "./pricing";
  import PricingRow from "./PricingRow.svelte";
  import RoleForm from "./RoleForm.svelte";
  import StaffForm from "./StaffForm.svelte";
  import {
    allowedOf,
    canDeactivate,
    isBuiltIn,
    matches,
    people,
    proposeDeactivate,
    roleEditable,
    type Person,
  } from "./staff";

  let { data, scope }: { data: BranchData[]; scope: string } = $props();

  type Mode =
    { kind: "staff"; person: Person | null } | { kind: "role"; item: Listed | null } | null;
  let mode: Mode = $state(null);
  let query = $state("");

  const team = $derived(people(data).filter((p) => matches(p, query)));
  const roles = $derived(listed(data, (c) => c.roles));

  // The form closes once its change was sent or queued.
  let seen = cmd.sent;
  $effect(() => {
    if (cmd.sent !== seen) {
      seen = cmd.sent;
      mode = null;
    }
  });
</script>

<TextField label="owner.staff.search" type="search" bind:value={query} />

<section class="panel" aria-labelledby="st-team">
  <h2 id="st-team" class="type-hub-title">{t("owner.staff.title")}</h2>
  {#if !team.length}
    <p>{t("owner.staff.empty")}</p>
  {:else}
    <ul class="rows">
      {#each team as p (p.key)}
        <PricingRow
          kind="staff"
          name={p.name}
          edit={p.members.some((m) => m.staff.login)
            ? {
                label: t("owner.staff.edit", { name: p.name }),
                onclick: () => (mode = { kind: "staff", person: p }),
              }
            : undefined}
          more={canDeactivate(p)
            ? {
                label: t("owner.staff.deactivate", { name: p.name }),
                onclick: () => void propose(proposeDeactivate({ scope, data }, p)),
              }
            : undefined}
        >
          <span class="type-hub-label">{p.name}</span>
          {#if p.login}<span class="muted type-hub-micro">{p.login}</span>{/if}
          <ul>
            {#each p.members as m (m.data.branch.id)}
              <li class="muted type-hub-micro">
                {t("owner.staff.at", { branch: m.data.branch.name, role: m.staff.role })}
                {#if !m.staff.active}
                  <StatusBadge tone="muted" text={t("owner.staff.inactive")} />
                {/if}
              </li>
            {/each}
          </ul>
        </PricingRow>
      {/each}
    </ul>
  {/if}
</section>

<section class="panel" aria-labelledby="st-roles">
  <h2 id="st-roles" class="type-hub-title">{t("owner.roles.title")}</h2>
  <ul class="rows">
    {#each roles as r (r.key)}
      {@const source = sourceOf(r)}
      <PricingRow
        kind="role"
        name={r.name}
        edit={source && roleEditable(source.row)
          ? {
              label: t("owner.roles.edit", { name: r.name }),
              onclick: () => (mode = { kind: "role", item: r }),
            }
          : undefined}
      >
        <span class="type-hub-label">{r.name}</span>
        {#if source}
          <span class="muted type-hub-micro">
            {#if isBuiltIn(source.row)}{t("owner.roles.builtin")} ·
            {/if}{t("owner.roles.count", { n: allowedOf(source.row).length })}
          </span>
          <details>
            <summary class="type-hub-micro">{t("owner.roles.show", { name: r.name })}</summary>
            <ul class="perms">
              {#each allowedOf(source.row) as perm (perm)}
                <li class="type-hub-micro"><code>{perm}</code></li>
              {/each}
            </ul>
          </details>
        {/if}
      </PricingRow>
    {/each}
  </ul>
</section>

{#if mode?.kind === "staff"}
  {#key mode.person?.key ?? ""}
    <StaffForm person={mode.person} {data} {scope} oncancel={() => (mode = null)} />
  {/key}
{:else if mode?.kind === "role"}
  {#key mode.item?.key ?? ""}
    <RoleForm item={mode.item} {data} {scope} oncancel={() => (mode = null)} />
  {/key}
{:else}
  <div class="add">
    <Button
      label="owner.staff.add"
      variant="secondary"
      onclick={() => (mode = { kind: "staff", person: null })}
    />
    <Button
      label="owner.roles.add"
      variant="secondary"
      onclick={() => (mode = { kind: "role", item: null })}
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
  .perms {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
