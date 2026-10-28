<script lang="ts">
  import { Button, PairingCode, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { PairingCodeIssued } from "@arena/ui/generated";
  import { PUBLIC_ANDROID_HUB_URL, PUBLIC_HUB_DOWNLOAD_URL } from "$app/env/public";
  import { onMount, tick } from "svelte";
  import { getInstallHub, issuePairingCode, type ApiFailure } from "./api";
  import SetupStartPanel from "./SetupStartPanel.svelte";
  import { STEP_NAMES } from "./wizard.svelte";

  const id = $props.id();
  const downloadUrl = PUBLIC_HUB_DOWNLOAD_URL || "/download/hub";
  const playUrl =
    PUBLIC_ANDROID_HUB_URL || "https://play.google.com/store/apps/details?id=in.arena360.hub";

  let issued: PairingCodeIssued | null = $state(null);
  let paired = $state(false);
  // Free runs on the Android Hub only; paid and comp branches keep Windows first. `null` until
  // the tier is known, so a paid owner never sees the Android-only block while loading.
  let free: boolean | null = $state(null);
  let loading = $state(true);
  let loadError: string | null = $state(null);
  let regenerating = $state(false);
  let regenError: string | null = $state(null);
  let status: MessageKey | null = $state(null);

  function minSec(totalS: number): string {
    const s = Math.max(1, Math.ceil(totalS));
    return [String(Math.floor(s / 60)), String(s % 60).padStart(2, "0")].join(":");
  }

  function failure(f: ApiFailure): string {
    if (f.error.code === "STEP_NOT_READY") return t("onboarding.install.error.not_ready");
    if (f.error.code === "RATE_LIMITED") {
      return t("onboarding.install.error.rate_limited", { time: minSec(f.retryAfterS ?? 60) });
    }
    return t(f.offline ? "onboarding.error.offline" : "onboarding.error.generic");
  }

  const refused = (f: ApiFailure) => !f.offline && f.status >= 400 && f.status < 500;

  async function load() {
    loading = true;
    loadError = null;
    const state = await getInstallHub();
    if (!state.ok) {
      loadError = failure(state);
      loading = false;
      return;
    }
    free = state.data.tier === "free";
    if (state.data.hub_paired) {
      paired = true;
      loading = false;
      return;
    }
    const res = await issuePairingCode();
    loading = false;
    if (!res.ok) {
      loadError = failure(res);
      return;
    }
    issued = res.data;
  }

  async function regenerate() {
    if (regenerating) return;
    regenerating = true;
    regenError = null;
    status = null;
    const res = await issuePairingCode();
    regenerating = false;
    if (!res.ok) {
      if (refused(res)) {
        regenError = failure(res);
        return;
      }
      issued = null;
      await load();
      return;
    }
    issued = res.data;
    status = "onboarding.install.new_code";
    await tick();
    document.querySelector<HTMLElement>('[data-component="pairing-code"] [tabindex="-1"]')?.focus();
  }

  onMount(load);
</script>

<h1 tabindex="-1">{t(STEP_NAMES.install_hub)}</h1>
<p class="intro">
  {free === null ? "" : t(free ? "onboarding.install.intro_android" : "onboarding.install.intro")}
</p>

<SetupStartPanel />

<ol class="parts">
  <li class="part">
    <h2 id="{id}-download">{t("onboarding.install.download.title")}</h2>
    {#if free === null}
      <!-- the tier is not known yet -->
    {:else if free}
      <p>{t("onboarding.install.download.platform_android")}</p>
      <p class="spec" data-testid="hub-spec">{t("onboarding.install.download.spec_android")}</p>
      <div class="links">
        <a
          class="download"
          href={playUrl}
          target="_blank"
          rel="noopener external"
          data-testid="hub-download-android"
        >
          {t("onboarding.install.download.link_android")}
        </a>
      </div>
    {:else}
      <p>{t("onboarding.install.download.platform")}</p>
      <div class="links">
        <a class="download" href={downloadUrl} rel="external" data-testid="hub-download">
          {t("onboarding.install.download.link")}
        </a>
        <a class="link" href="/help/android-hub">{t("onboarding.install.download.android")}</a>
      </div>
    {/if}
  </li>
  <li class="part">
    <h2 id="{id}-install">{t("onboarding.install.install.title")}</h2>
    {#if free !== null}
      <p>
        {t(free ? "onboarding.install.install.body_android" : "onboarding.install.install.body")}
      </p>
    {/if}
  </li>
  <li class="part">
    <h2 id="{id}-code">{t("onboarding.install.code.title")}</h2>
    {#if paired}
      <p>{t("onboarding.install.paired")}</p>
    {:else if loadError}
      <p class="form-error" role="alert">
        <span class="glyph" aria-hidden="true">⚠</span><span>{loadError}</span>
      </p>
      <div class="actions">
        <Button label="onboarding.account_setup.retry" onclick={load} />
      </div>
    {:else if loading || !issued}
      <p role="status">{t("onboarding.install.loading")}</p>
    {:else}
      <p>{t("onboarding.install.code.body")}</p>
      <PairingCode
        code={issued.code}
        expiresAt={issued.expires_at}
        serverNow={issued.server_now}
        onregenerate={regenerate}
        {regenerating}
      />
      {#if regenError}
        <p class="form-error" role="alert">
          <span class="glyph" aria-hidden="true">⚠</span><span>{regenError}</span>
        </p>
      {/if}
      <p class="visually-hidden" role="status" data-testid="install-status">
        {#if status}{t(status)}{/if}
      </p>
      <p class="waiting">{t("onboarding.install.waiting")}</p>
    {/if}
  </li>
</ol>

<style>
  .intro {
    margin: 0 0 var(--space-5);
    line-height: var(--type-hub-label-line-height);
  }

  .parts {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .part {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }

  .part h2 {
    margin: 0;
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .part p {
    margin: 0;
    line-height: var(--type-hub-label-line-height);
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
  }

  .download {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    max-width: 100%;
    padding: var(--space-2) var(--space-5);
    border-radius: var(--radius-full);
    border: 1px solid var(--border-strong);
    background-color: var(--button-secondary-background);
    color: var(--button-secondary-foreground);
    font-weight: var(--type-hub-title-weight);
    text-decoration: none;
    overflow-wrap: anywhere;
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    max-width: 100%;
    color: var(--accent-text);
    text-decoration: underline;
    overflow-wrap: anywhere;
  }

  .spec {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }

  .waiting {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }

  .form-error {
    display: flex;
    gap: var(--space-2);
    color: var(--text-field-error-text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
