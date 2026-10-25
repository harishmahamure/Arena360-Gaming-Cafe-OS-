<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { revealSetupCode } from "./api";
  import { setupCode } from "./wizard.svelte";

  let status: MessageKey | null = $state(null);
  let valueEl: HTMLElement | undefined = $state();
  const grouped = $derived(
    setupCode.value ? [setupCode.value.slice(0, 4), setupCode.value.slice(4)].join(" ") : "",
  );

  onMount(async () => {
    if (setupCode.asked) return;
    setupCode.asked = true;
    const res = await revealSetupCode();
    if (res.ok) setupCode.value = res.data.code;
    else if (res.error.code === "INTERNAL") setupCode.asked = false;
  });

  async function copy() {
    try {
      await navigator.clipboard.writeText(setupCode.value ?? "");
      status = "ui.pairing_code.copied";
    } catch {
      const range = document.createRange();
      if (valueEl) range.selectNodeContents(valueEl);
      window.getSelection()?.removeAllRanges();
      window.getSelection()?.addRange(range);
      status = "ui.pairing_code.copy_hint";
    }
  }
</script>

{#if setupCode.value}
  <section class="notice" aria-labelledby="setup-code-title" data-testid="setup-code-notice">
    <h2 id="setup-code-title">{t("onboarding.setup_code.title")}</h2>
    <p class="value" bind:this={valueEl} data-testid="setup-code-text">
      {t("onboarding.setup_code.text", { code: grouped })}
    </p>
    <p class="once">{t("onboarding.setup_code.once")}</p>
    <div class="actions">
      <Button label="ui.pairing_code.copy" variant="secondary" onclick={copy} />
    </div>
    <p class="status" role="status">
      {#if status}{t(status)}{/if}
    </p>
  </section>
{/if}

<style>
  .notice {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0 0 var(--space-5);
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
  }

  h2 {
    margin: 0;
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  p {
    margin: 0;
    line-height: var(--type-hub-label-line-height);
  }

  .value {
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }

  .once,
  .status {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
