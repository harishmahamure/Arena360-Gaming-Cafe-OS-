<script lang="ts">
  import { Button, TextField, t } from "@arena/ui";
  import type { SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { newIdempotencyKey, submitIntent, hasPermission } from "./auth";
  import { MESSAGE_MAX, messageValid } from "./tile-actions.svelte";

  // "Message all PCs" (Story 6.16): one text to every online PC that has a live session. The
  // Hub says how many PCs it reached. A retry after a failure resends the same idempotency key.

  let { staff }: { staff: SessionStaff | null } = $props();

  let open = $state(false);
  let text = $state("");
  let error: MessageKey | null = $state(null);
  let sending = $state(false);
  let result: { text: string; failed: boolean } | null = $state(null);
  let key = newIdempotencyKey();
  let keyedText = "";
  let field: HTMLElement | undefined = $state();

  const allowed = $derived(hasPermission(staff, "device.screen"));

  function toggle() {
    open = !open;
    result = null;
    error = null;
    if (open) queueMicrotask(() => field?.querySelector("input")?.focus());
  }

  async function send(event: SubmitEvent) {
    event.preventDefault();
    if (sending) return;
    if (!messageValid(text)) {
      error = "hub.sheet.message_invalid";
      return;
    }
    error = null;
    sending = true;
    result = null;
    if (text.trim() !== keyedText) key = newIdempotencyKey();
    keyedText = text.trim();
    const outcome = await submitIntent(
      { kind: "device.message", scope: "all", text: text.trim() },
      key,
    );
    sending = false;
    if (outcome.kind === "applied") {
      const n = outcome.result.delivered ?? 0;
      result = {
        text:
          n === 0
            ? t("hub.floor.message_all_none")
            : n === 1
              ? t("hub.floor.message_all_sent_one")
              : t("hub.floor.message_all_sent", { count: n }),
        failed: false,
      };
      text = "";
      key = newIdempotencyKey();
    } else if (outcome.kind === "error" && outcome.error.field === "text") {
      error = "hub.sheet.message_invalid";
    } else if (outcome.kind === "error") {
      result = { text: t("hub.floor.message_all_refused"), failed: true };
      key = newIdempotencyKey();
    } else if (outcome.kind === "unreachable") {
      result = { text: t("hub.floor.message_all_failed"), failed: true };
    }
  }
</script>

{#if allowed}
  <section class="message-all" data-testid="message-all">
    <Button label="hub.floor.message_all" variant="secondary" onclick={toggle} />
    {#if open}
      <form class="compose" onsubmit={send} bind:this={field} data-testid="message-all-form">
        <TextField label="hub.floor.message_all_label" bind:value={text} {error} />
        <span class="count type-hub-label tabular-nums"
          >{t("hub.sheet.message_count", { count: [...text].length, max: MESSAGE_MAX })}</span
        >
        <Button
          label="hub.floor.message_all_send"
          type="submit"
          disabled={sending}
          disabledReason="hub.floor.message_all_sending"
        />
      </form>
    {/if}
    <p
      class="result type-hub-body"
      class:failed={result?.failed}
      role="status"
      data-testid="message-all-result"
    >
      {#if result}{result.text}{/if}
    </p>
  </section>
{/if}

<style>
  .message-all {
    display: grid;
    justify-items: start;
    gap: var(--space-3);
  }

  .compose {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
    width: min(100%, 32rem);
  }

  .count {
    color: var(--text-muted);
  }

  .result {
    margin: 0;
  }

  .result:empty {
    display: none;
  }

  .failed {
    color: var(--danger);
  }
</style>
