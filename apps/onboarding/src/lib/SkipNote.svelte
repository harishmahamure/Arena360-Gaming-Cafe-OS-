<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { ApiResult } from "./api";
  import { advance } from "./wizard.svelte";
  import type { WizardState } from "@arena/ui/generated";

  let { skip, note }: { skip: () => Promise<ApiResult<WizardState>>; note: MessageKey } = $props();
  let busy = false;
  let error: MessageKey | null = $state(null);

  async function go() {
    if (busy) return;
    busy = true;
    const res = await skip();
    busy = false;
    if (res.ok) return advance(res.data);
    error = res.offline ? "onboarding.error.offline" : "onboarding.error.generic";
  }
</script>

{#if error}
  <p class="form-error" role="alert"><span aria-hidden="true">⚠</span> {t(error)}</p>
{/if}
<div class="skip">
  <Button label="onboarding.skip" variant="secondary" onclick={go} />
  <p>{t(note)}</p>
</div>

<style>
  .skip {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-5);
  }

  p {
    margin: 0;
  }

  .form-error {
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }
</style>
