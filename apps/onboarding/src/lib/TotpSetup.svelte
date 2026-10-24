<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { TotpStarted } from "@arena/ui/generated";

  /** The authenticator-app QR code and its setup key (grouped by 4) with "Copy setup key". */
  let { totp }: { totp: TotpStarted } = $props();

  let status: MessageKey | null = $state(null);
  let error: MessageKey | null = $state(null);

  const src = $derived("data:image/svg+xml;charset=utf-8," + encodeURIComponent(totp.qr_svg));
  const grouped = $derived(totp.secret.match(/.{1,4}/g)?.join(" ") ?? totp.secret);

  async function copy() {
    error = null;
    try {
      await navigator.clipboard.writeText(totp.secret);
      status = "onboarding.totp.copied_key";
    } catch {
      status = null;
      error = "onboarding.copy_failed";
    }
  }
</script>

<div class="totp">
  <img class="qr" {src} alt={t("onboarding.totp.qr_alt")} width="240" height="240" />
  <div class="key">
    <p class="key-label" id="totp-key-label">{t("onboarding.totp.key_label")}</p>
    <p class="key-value" aria-labelledby="totp-key-label" data-testid="totp-secret">
      <code>{grouped}</code>
    </p>
    <p class="help">{t("onboarding.totp.key_help")}</p>
    <Button label="onboarding.totp.copy_key" variant="secondary" onclick={copy} />
    <p class="status" role="status">
      {#if status}{t(status)}{/if}
    </p>
    <p class="error" role="alert">
      {#if error}<span class="glyph" aria-hidden="true">⚠</span><span>{t(error)}</span>{/if}
    </p>
  </div>
</div>

<style>
  .totp {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-5);
    align-items: flex-start;
  }

  .qr {
    flex: none;
    width: 240px;
    max-width: 100%;
    height: auto;
    border-radius: var(--radius-md);
    /* The QR code keeps its light quiet zone in either theme. */
    background: var(--color-surface-light);
  }

  .key {
    display: flex;
    flex: 1 1 14rem;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }

  .key-label {
    margin: 0;
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .key-value {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .key-value code {
    font-family: ui-monospace, monospace;
    font-size: var(--type-web-body-size);
    letter-spacing: 0.04em;
  }

  .help {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .status,
  .error {
    margin: 0;
    min-height: 1lh;
  }

  .error {
    display: flex;
    gap: var(--space-2);
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }
</style>
