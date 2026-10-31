<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { HandoverView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY } from "./auth";
  import { handoverStep, type HandoverResult, type HandoverStep } from "./handover";
  import { pushBack } from "./back";
  import { SNAPSHOT_DEPENDENCY } from "./snapshot";

  // "Move this branch to <PC name>" on the Android Hub (Story 10.17): an Owner or Manager confirms with
  // their PIN and the tablet shows a 6-digit code; the PC asks for it. "Use a file instead" and
  // "Cancel move" ask for the PIN again.
  let {
    pc,
    view,
    addresses,
    onclose,
  }: {
    pc: string;
    /** The handover as the last snapshot says, while the code shows. */
    view: HandoverView | null;
    /** This tablet's LAN addresses, for the PC to type. */
    addresses: string[];
    onclose: () => void;
  } = $props();

  type PinResult = { ok: true } | { ok: false; message: MessageKey; retryAfterS?: number };

  let asking: HandoverStep | null = $state(null);
  let problem: MessageKey | null = $state(null);
  let section: HTMLElement | undefined = $state();

  const PROMPTS: Record<HandoverStep, MessageKey> = {
    start: "hub.handover.pin_prompt",
    file: "hub.handover.file_pin_prompt",
    cancel: "hub.handover.cancel_pin_prompt",
  };

  function reasonOf(result: Extract<HandoverResult, { kind: "refused" }>): MessageKey {
    const folder = result.error.code === "VALIDATION_FAILED" && result.error.field === "folder";
    return folder ? "hub.handover.error.folder" : "hub.handover.error.generic";
  }

  async function submit(step: HandoverStep, pin: string): Promise<PinResult> {
    const result = await handoverStep(step, pin);
    switch (result.kind) {
      case "ok":
        asking = null;
        problem = null;
        // The snapshot carries the new view; fetch it now rather than at the next poll.
        await invalidate(SNAPSHOT_DEPENDENCY);
        if (step === "cancel") onclose();
        return { ok: true };
      case "rejected":
        return { ok: false, message: "ui.pin.rejected" };
      case "locked":
        return { ok: false, message: "ui.pin.locked", retryAfterS: result.retryAfterS };
      case "signed_out":
        void invalidate(SESSION_DEPENDENCY);
        return { ok: false, message: "ui.pin.unreachable" };
      case "refused":
        asking = null;
        problem = reasonOf(result);
        return { ok: true };
      default:
        return { ok: false, message: "ui.pin.unreachable" };
    }
  }

  onMount(() => {
    section?.focus();
    return pushBack(() => onclose());
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  class="move"
  bind:this={section}
  tabindex="-1"
  aria-labelledby="move-title"
  data-testid="move-branch"
  onkeydown={(e) => e.key === "Escape" && onclose()}
>
  <h2 class="type-hub-title" id="move-title">{t("hub.handover.title", { pc })}</h2>

  {#if view?.code}
    <p class="type-hub-body">{t("hub.handover.code_steps", { pc })}</p>
    <p class="type-hub-label muted">{t("hub.handover.code_label")}</p>
    <p class="code type-hub-numeric" data-testid="handover-code" aria-live="polite">
      {view.code}
    </p>
    {#if addresses.length > 0}
      <p class="type-hub-body" data-testid="handover-address">
        {t("hub.handover.address_label")}: <strong>{addresses.join(", ")}</strong>
      </p>
    {/if}
    <p class="type-hub-label muted">{t("hub.handover.stops_note")}</p>
    <div class="row">
      <Button label="hub.handover.use_file" variant="secondary" onclick={() => (asking = "file")} />
      <Button
        label="hub.handover.cancel_move"
        variant="secondary"
        onclick={() => (asking = "cancel")}
      />
    </div>
  {:else}
    <p class="type-hub-body">{t("hub.handover.explain", { pc })}</p>
    <ol class="type-hub-body">
      <li>{t("hub.handover.step_confirm")}</li>
      <li>{t("hub.handover.step_code", { pc })}</li>
      <li>{t("hub.handover.step_wait")}</li>
    </ol>
    <div class="row">
      <Button label="hub.handover.show_code" onclick={() => (asking = "start")} />
    </div>
  {/if}

  {#if asking}
    <div class="approval" data-testid="handover-approval">
      <InlinePin
        message={PROMPTS[asking]}
        onsubmit={(pin) => submit(asking!, pin)}
        oncancel={() => (asking = null)}
      />
    </div>
  {/if}

  {#if problem}
    <p class="type-hub-label warn" role="alert" data-testid="handover-problem">
      <span aria-hidden="true">⚠</span>
      <span>{t(problem)}</span>
    </p>
  {/if}

  <div class="row">
    <Button label="hub.handover.close" variant="secondary" onclick={onclose} />
  </div>
</section>

<style>
  .move {
    position: fixed;
    inset-block-start: var(--space-8);
    inset-inline: 0;
    margin-inline: auto;
    z-index: 8;
    box-sizing: border-box;
    display: grid;
    gap: var(--space-3);
    width: min(32rem, calc(100vw - 2 * var(--space-3)));
    max-height: 80dvh;
    overflow-y: auto;
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--hub-header-background);
    color: var(--text);
  }

  h2,
  p,
  ol {
    margin: 0;
  }

  ol {
    padding-inline-start: var(--space-4);
  }

  .code {
    letter-spacing: 0.3em;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .muted {
    color: var(--text-muted);
  }

  .warn {
    display: flex;
    gap: var(--space-2);
  }
</style>
