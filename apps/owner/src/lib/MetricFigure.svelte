<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";

  /** An Owner app figure: label, value and the age of the number. */
  let {
    label,
    value,
    age,
    late = false,
    muted = false,
    children,
  }: {
    label: MessageKey;
    value?: string;
    age?: string;
    late?: boolean;
    muted?: boolean;
    children?: Snippet;
  } = $props();
</script>

<div class="figure" class:muted data-component="metric-figure">
  <dt class="type-hub-label">{t(label)}</dt>
  <dd>
    {#if children}{@render children()}{:else}<span class="value tabular-nums">{value}</span>{/if}
    {#if age}
      <span class="age type-hub-micro" class:late
        >{age}{#if late}&nbsp;·&nbsp;{t("owner.today.age.late")}{/if}</span
      >
    {/if}
  </dd>
</div>

<style>
  .figure {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
  }
  dt {
    color: var(--text-muted);
  }
  dd {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .value {
    font-family: var(--type-hub-numeric-family);
    font-size: var(--type-hub-numeric-size);
    font-weight: var(--type-hub-numeric-weight);
    line-height: var(--type-hub-numeric-line-height);
    color: var(--metric-figure, var(--text));
  }
  .age {
    color: var(--text-muted);
  }
  .age.late {
    color: var(--warning-text, var(--warning));
    font-weight: 700;
  }
  .muted .value,
  .muted dd {
    color: var(--text-muted);
  }
</style>
