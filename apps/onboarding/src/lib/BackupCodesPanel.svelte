<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";

  /**
   * The backup codes, shown once: the list, "Copy codes", "Download .txt" and the required
   * "I've saved my backup codes" box that enables Continue. `onsave` acks them and answers
   * whether that worked.
   */
  let {
    codes,
    onsave,
    continueLabel = "onboarding.codes.continue",
  }: {
    codes: string[];
    onsave: () => Promise<boolean>;
    continueLabel?: MessageKey;
  } = $props();

  const FILE_NAME = "arena-360-backup-codes.txt"; // i18n-ignore

  const id = $props.id();
  let saved = $state(false);
  let saving = $state(false);
  let status: MessageKey | null = $state(null);
  let error: MessageKey | null = $state(null);

  const text = $derived([t("onboarding.codes.file_header"), "", ...codes, ""].join("\n"));

  async function copy() {
    error = null;
    try {
      await navigator.clipboard.writeText(codes.join("\n"));
      status = "onboarding.codes.copied";
    } catch {
      status = null;
      error = "onboarding.copy_failed";
    }
  }

  function download() {
    error = null;
    const url = URL.createObjectURL(new Blob([text], { type: "text/plain;charset=utf-8" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = FILE_NAME;
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    status = "onboarding.codes.downloaded";
  }

  async function save() {
    if (!saved || saving) return;
    saving = true;
    error = null;
    const ok = await onsave();
    saving = false;
    if (!ok) error = "onboarding.error.generic";
  }
</script>

<div class="panel" data-testid="backup-codes">
  <ol class="codes" aria-label={t("onboarding.codes.list_label")}>
    {#each codes as code (code)}
      <li><code>{code}</code></li>
    {/each}
  </ol>
  <div class="actions">
    <Button label="onboarding.codes.copy" variant="secondary" onclick={copy} />
    <Button label="onboarding.codes.download" variant="secondary" onclick={download} />
  </div>
  <p class="status" role="status">
    {#if status}{t(status)}{/if}
  </p>
  <div class="saved">
    <span class="check">
      <input id="{id}-saved" type="checkbox" required bind:checked={saved} />
      <span class="box" aria-hidden="true">{saved ? "✓" : ""}</span>
    </span>
    <label for="{id}-saved">{t("onboarding.codes.saved")}</label>
  </div>
  <p class="error" role="alert">
    {#if error}<span class="glyph" aria-hidden="true">⚠</span><span>{t(error)}</span>{/if}
  </p>
  <div class="actions">
    <Button
      label={continueLabel}
      onclick={save}
      disabled={!saved || saving}
      disabledReason={saving ? "onboarding.codes.saving" : "onboarding.codes.confirm_first"}
    />
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .codes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(9rem, 1fr));
    gap: var(--space-2) var(--space-4);
    margin: 0;
    padding: var(--space-4) var(--space-4) var(--space-4) var(--space-7);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface-2);
  }

  .codes code {
    font-family: ui-monospace, monospace;
    font-size: var(--type-web-body-size);
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.04em;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
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

  .saved {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .saved label {
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  /* A 44 × 44 px target: the real checkbox covers the box, transparent. */
  .check {
    position: relative;
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
  }

  .check input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .box {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 1.5rem;
    height: 1.5rem;
    border: 2px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--text-field-background);
    color: var(--text-field-foreground);
    font-weight: var(--type-hub-title-weight);
    pointer-events: none;
  }

  .check:has(input:checked) .box {
    border-color: var(--accent);
  }

  .check:has(input:focus-visible) .box {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
