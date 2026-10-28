<script lang="ts">
  import type { MessageKey } from "@arena/ui/messages";
  import { t } from "@arena/ui";
  import type { Snippet } from "svelte";

  /**
   * A wide table that scrolls sideways on a narrow screen. The region is focusable so a keyboard
   * user can scroll it, and named so a screen reader knows what it holds.
   */
  let { label, children }: { label: MessageKey; children: Snippet } = $props();
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="scroll" role="region" tabindex="0" aria-label={t(label)}>
  {@render children()}
</div>

<style>
  .scroll {
    overflow-x: auto;
  }
</style>
