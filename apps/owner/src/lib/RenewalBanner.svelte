<script lang="ts">
  import { formatDateTime, t } from "@arena/ui";
  import { shell, unpaidPlans } from "./shell.svelte";
  import { unpaidDate } from "./subscription";

  // A persistent warning on every screen while a renewal waits for approval or is unpaid (Story
  // 10.11). It names the branch, the state and the date, and leads to that branch's card.
  const rows = $derived(
    unpaidPlans().map((plan) => {
      const timezone = shell.branches?.find((b) => b.id === plan.branch_id)?.timezone;
      const at = unpaidDate(plan);
      const branch = plan.branch_name;
      const grace = plan.state === "grace";
      return {
        id: plan.branch_id,
        text: t(grace ? "owner.renewal.banner.grace" : "owner.renewal.banner.needs_approval", {
          branch,
          date: at === undefined ? "" : formatDateTime(at, timezone, { dateStyle: "medium" }),
        }),
        action: t(grace ? "owner.renewal.banner.update" : "owner.renewal.banner.approve"),
        label: t(grace ? "owner.renewal.banner.update_for" : "owner.renewal.banner.approve_for", {
          branch,
        }),
        href: `/account/subscription?branch=${encodeURIComponent(plan.branch_id)}`,
      };
    }),
  );
</script>

{#if rows.length}
  <section class="banner" aria-label={t("owner.renewal.banner.label")} data-testid="renewal-banner">
    <ul>
      {#each rows as row (row.id)}
        <li>
          <span>{row.text}</span>
          <a class="ui-interactive" href={row.href} aria-label={row.label}>{row.action}</a>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .banner {
    margin-bottom: var(--space-4);
    padding: var(--space-3);
    border: 2px solid var(--warning-text, var(--warning));
    border-radius: var(--radius-md);
    background-color: var(--surface);
    color: var(--text);
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  a {
    font-weight: var(--type-hub-label-weight);
    color: var(--warning-text, var(--warning));
    text-decoration: underline;
  }
</style>
