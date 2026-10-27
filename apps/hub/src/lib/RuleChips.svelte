<script lang="ts">
  import { t } from "@arena/ui";
  import type { RuleChip } from "./rules";

  let { chips, scope }: { chips: RuleChip[]; scope: string } = $props();
</script>

{#if chips.length > 0}
  <ul class="rule-chips" aria-label={t("hub.rules.chip.label")} data-testid="{scope}-rule-chips">
    {#each chips as chip (chip.key)}
      <li class="chip-rule type-hub-label" data-testid="chip-rule">
        <span class="chip-glyph" aria-hidden="true">⚡</span>
        {chip.text}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .rule-chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* A rule in effect: tinted, with a glyph and words (never colour alone). */
  .chip-rule {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-full);
    background-color: var(--chip-rule-background);
    color: var(--chip-rule-foreground);
    white-space: nowrap;
  }
</style>
