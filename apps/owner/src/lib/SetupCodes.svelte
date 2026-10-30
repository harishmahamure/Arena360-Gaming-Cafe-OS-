<script lang="ts">
  import { Button, Card, t } from "@arena/ui";
  import { branchOf, cmd, dismissCodes } from "./commands.svelte";

  // Eight characters read as two groups of four, like the Hub shows them.
  const spaced = (code: string) => `${code.slice(0, 4)} ${code.slice(4)}`; // i18n-ignore
  let copied = $state("");

  async function copy(code: string, key: string) {
    try {
      await navigator.clipboard.writeText(code);
      copied = key;
    } catch {
      copied = "";
    }
  }
</script>

{#if cmd.codes.length}
  <Card>
    <section
      class="codes"
      aria-labelledby="setup-codes-title"
      data-component="setup-codes"
      role="status"
    >
      <h2 id="setup-codes-title" class="type-hub-title">{t("owner.staff.code.title")}</h2>
      <p class="hint">{t("owner.staff.code.once")}</p>
      <ul>
        {#each cmd.codes as c (c.branch_id + c.staff_id)}
          {@const key = c.branch_id + c.staff_id}
          <li>
            <p class="line">
              {t("owner.staff.code.shown", { name: c.name, code: spaced(c.code) })}
            </p>
            <p class="where type-hub-micro">{branchOf(c.branch_id)?.name ?? c.branch_id}</p>
            <button
              type="button"
              class="ui-interactive btn type-hub-label"
              onclick={() => copy(c.code, key)}
              >{t("owner.staff.code.copy", { name: c.name })}</button
            >
            {#if copied === key}<span class="type-hub-label">{t("owner.staff.code.copied")}</span
              >{/if}
          </li>
        {/each}
      </ul>
      <div>
        <Button label="owner.staff.code.done" variant="secondary" onclick={dismissCodes} />
      </div>
    </section>
  </Card>
{/if}

<style>
  .codes {
    display: grid;
    gap: var(--space-3);
  }
  h2,
  p {
    margin: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-3);
  }
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }
  .line {
    font-weight: var(--type-hub-title-weight);
    overflow-wrap: anywhere;
  }
  .hint,
  .where {
    color: var(--text-muted);
  }
  .btn {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
</style>
