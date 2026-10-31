<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { HandoverView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, getToken } from "./auth";
  import { handoverStep, type HandoverStep } from "./handover";
  import { SNAPSHOT_DEPENDENCY } from "./snapshot";

  // The Android Hub after a PC claimed the handover code (Story 10.17): it takes no changes and serves
  // nothing. While the snapshot moves it says so; with a file it names the file; once the PC holds the
  // snapshot it only says the branch has moved. Until then "Cancel move" (PIN) resumes it as main.
  let { view }: { view: HandoverView } = $props();

  type PinResult = { ok: true } | { ok: false; message: MessageKey; retryAfterS?: number };

  let asking: HandoverStep | null = $state(null);
  let problem: MessageKey | null = $state(null);
  const pc = $derived(view.target_hub_name);
  const signedIn = $derived(getToken() !== null);

  async function submit(step: HandoverStep, pin: string): Promise<PinResult> {
    const result = await handoverStep(step, pin);
    switch (result.kind) {
      case "ok":
        asking = null;
        problem = null;
        await invalidate(SNAPSHOT_DEPENDENCY);
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
        problem =
          result.error.code === "VALIDATION_FAILED" && result.error.field === "folder"
            ? "hub.handover.error.folder"
            : "hub.handover.error.generic";
        return { ok: true };
      default:
        return { ok: false, message: "ui.pin.unreachable" };
    }
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.handover.stopped.title", { pc }) })}</title>
</svelte:head>

<section class="stopped" aria-labelledby="stopped-title" data-testid="hub-handover-stopped">
  {#if view.phase === "moved"}
    <h1 class="type-hub-numeric" id="stopped-title">{t("hub.handover.moved.title", { pc })}</h1>
    <p class="type-hub-body">{t("hub.handover.moved.body", { pc })}</p>
  {:else}
    <h1 class="type-hub-numeric" id="stopped-title">{t("hub.handover.stopped.title", { pc })}</h1>
    {#if view.phase === "file" && view.file_name}
      <p class="type-hub-body" data-testid="handover-file">
        {t("hub.handover.stopped.file", { file: view.file_name, pc })}
      </p>
    {:else}
      <p class="type-hub-body">{t("hub.handover.stopped.transferring", { pc })}</p>
      <div class="row">
        <Button
          label="hub.handover.use_file"
          variant="secondary"
          disabled={!signedIn}
          disabledReason="hub.handover.sign_in_first"
          onclick={() => (asking = "file")}
        />
      </div>
    {/if}
    {#if view.last_seq !== undefined && view.last_seq !== null}
      <p class="type-hub-label muted">{t("hub.handover.last_change", { seq: view.last_seq })}</p>
    {/if}
    <div class="row">
      <Button
        label="hub.handover.cancel_move"
        variant="secondary"
        disabled={!signedIn}
        disabledReason="hub.handover.sign_in_first"
        onclick={() => (asking = "cancel")}
      />
    </div>
    {#if asking}
      <div class="approval" data-testid="handover-approval">
        <InlinePin
          message={asking === "cancel"
            ? "hub.handover.cancel_pin_prompt"
            : "hub.handover.file_pin_prompt"}
          onsubmit={(pin) => submit(asking!, pin)}
          oncancel={() => (asking = null)}
        />
      </div>
    {/if}
    {#if problem}
      <p class="type-hub-label" role="alert" data-testid="handover-problem">
        <span aria-hidden="true">⚠</span>
        {t(problem)}
      </p>
    {/if}
  {/if}
</section>

<style>
  .stopped {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  h1,
  p {
    margin: 0;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
