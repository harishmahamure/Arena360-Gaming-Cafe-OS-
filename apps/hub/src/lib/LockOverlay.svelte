<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { pushBack } from "./back";

  type PinResult = { ok: true } | { ok: false; message: MessageKey; retryAfterS?: number };

  /**
   * The idle lock: a full-viewport modal over the Hub shell (which the layout makes `inert`
   * and keeps mounted). A PIN unlocks it in place; Esc only clears the PIN. Sign out stays
   * available, even while unlocking is paused.
   */
  let {
    name,
    hasPin = true,
    onunlock,
    onsignout,
  }: {
    /** The signed-in staff member's display name. */
    name: string;
    /** The member has a PIN (otherwise the screen says to sign out). */
    hasPin?: boolean;
    onunlock: (pin: string) => Promise<PinResult>;
    onsignout: () => void;
  } = $props();

  const id = $props.id();
  const titleId = id + "-title";

  // Android Back is swallowed while locked: it neither closes a layer under the lock nor exits.
  $effect(() => pushBack(() => {}));
</script>

<div class="lock" role="dialog" aria-modal="true" aria-labelledby={titleId} data-testid="hub-lock">
  <div class="panel">
    <h1 class="type-hub-numeric" id={titleId}>{t("hub.lock.title")}</h1>
    <p class="who type-hub-body" data-testid="hub-lock-staff">
      {t("hub.lock.signed_in", { name })}
    </p>
    {#if !hasPin}
      <p class="hint type-hub-label" data-testid="hub-lock-no-pin">{t("hub.lock.no_pin")}</p>
    {/if}
    <InlinePin
      message="hub.lock.prompt"
      label="hub.lock.pin_label"
      submitLabel="hub.lock.unlock"
      cancellable={false}
      onsubmit={onunlock}
      oncancel={() => {}}
    />
    <Button label="hub.header.sign_out" variant="secondary" onclick={onsignout} />
  </div>
</div>

<style>
  .lock {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    overflow-y: auto;
    padding: var(--space-hub-gutter-phone);
    background-color: var(--hub-bg);
    color: var(--text);
  }

  .panel {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 24rem;
  }

  h1,
  .who,
  .hint {
    margin: 0;
  }

  .hint {
    max-width: 24rem;
    color: var(--text);
  }

  .who {
    color: var(--text-muted);
  }
</style>
