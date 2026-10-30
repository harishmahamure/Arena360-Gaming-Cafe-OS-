<script module lang="ts">
  import type { BranchData } from "./pricing";

  /** What the board under the branch pick works with. */
  export type Scoped = {
    data: BranchData[];
    /** The one branch in scope, or `all`. */
    scope: string;
    now: number;
    /** Nothing could be read, and nothing is cached. */
    failed: boolean;
    /** Some branch has not answered yet. */
    waiting: boolean;
    /** Reads the branches again now. */
    retry: () => void;
  };
</script>

<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack, type Snippet } from "svelte";
  import { ALL, dataOf } from "./pricing";
  import { age, shell } from "./shell.svelte";
  import { TICK_MS, fetchDue, live, showCached, showable } from "./today.svelte";

  /**
   * The frame of a Manage board that edits the branches' catalogues (Pricing, Products): the
   * branch pick, each branch's data age and the reads that keep the catalogues current.
   */
  let {
    label,
    component,
    children,
  }: { label: MessageKey; component: string; children: Snippet<[Scoped]> } = $props();

  // The branches whose catalogues Manage can change: live and on a paid plan.
  const paid = $derived(showable(shell.branches));
  let branchId = $state(ALL);
  const picked = $derived(
    branchId === ALL || paid.length < 2 || !paid.some((b) => b.id === branchId)
      ? paid
      : paid.filter((b) => b.id === branchId),
  );
  // One branch in scope is that branch; several are "all branches".
  const scope = $derived(picked.length === 1 ? picked[0].id : ALL);
  const data = $derived(dataOf(picked));
  const waiting = $derived(picked.some((b) => !live[b.id]));
  const failed = $derived(picked.length > 0 && picked.every((b) => live[b.id]?.failed));

  let now = $state(Date.now());
  const ago = (ms: number) => {
    const [key, n] = age(new Date(ms).toISOString(), now);
    return t(key, { n });
  };

  async function read(force: boolean) {
    await showCached(paid);
    await fetchDue(paid, force);
  }

  onMount(() => {
    const id = setInterval(() => {
      now = Date.now();
      if (document.visibilityState === "visible") void read(false);
    }, TICK_MS);
    return () => clearInterval(id);
  });

  // The first read is forced; a branch that turns live or paid while the board is open is read at once.
  let first = true;
  $effect(() => {
    void paid.map((b) => b.id).join();
    const force = first;
    first = false;
    void untrack(() => read(force));
  });
</script>

{#if paid.length}
  <section class="scope" aria-label={t(label)} data-component={component}>
    {#if paid.length > 1}
      <div class="field">
        <label class="label" for="{component}-branch">{t("owner.pricing.branch")}</label>
        <select id="{component}-branch" class="ui-interactive select" bind:value={branchId}>
          <option value={ALL}>{t("owner.pricing.branch.all")}</option>
          {#each paid as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
        </select>
      </div>
    {/if}
    <ul class="ages">
      {#each data as d (d.branch.id)}
        <li class="type-hub-label" data-branch={d.branch.id}>
          {d.asOf === null
            ? t("owner.pricing.age_none", { branch: d.branch.name })
            : t("owner.pricing.age", { branch: d.branch.name, age: ago(d.asOf) })}
        </li>
      {/each}
    </ul>
    {@render children({ data, scope, now, failed, waiting, retry: () => void read(true) })}
  </section>
{/if}

<style>
  .scope {
    --status-badge-background: var(--surface);
    display: grid;
    gap: var(--space-4);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .label {
    font-weight: var(--type-hub-title-weight);
  }
  .select {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
    max-width: 100%;
  }
  .ages {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-1);
    color: var(--text-muted);
  }
</style>
