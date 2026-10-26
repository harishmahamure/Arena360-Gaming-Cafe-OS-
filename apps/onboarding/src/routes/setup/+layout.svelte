<script lang="ts">
  import { Button, WizardProgress, t } from "@arena/ui";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount, type Snippet } from "svelte";
  import { eventsUrl, getWizard } from "../../lib/api";
  import {
    advance,
    clearWizard,
    setWizard,
    stepPath,
    target,
    wizard,
  } from "../../lib/wizard.svelte";

  let { children }: { children: Snippet } = $props();

  let failed: "offline" | "generic" | null = $state(null);

  async function load() {
    failed = null;
    const res = await getWizard();
    if (res.ok) {
      setWizard(res.data);
      return;
    }
    if (res.error.code === "SETUP_INCOMPLETE") {
      await goto("/account-setup", { replaceState: true });
    } else if (res.error.code === "AUTH_FAILED") {
      await goto("/", { replaceState: true });
    } else {
      failed = res.offline ? "offline" : "generic";
    }
  }

  const POLL_MS = 5000;
  let streaming = false;

  async function sync() {
    const res = await getWizard();
    if (!res.ok) return;
    if (page.params.step === "install_hub" && res.data.current !== "install_hub") {
      await advance(res.data);
    } else {
      setWizard(res.data);
    }
  }

  function refresh() {
    if (document.visibilityState === "visible" && wizard.state) load();
  }

  onMount(() => {
    target.id = page.url.searchParams.get("branch") ?? "";
    clearWizard();
    load();
    const source = new EventSource(eventsUrl(), { withCredentials: true });
    source.onopen = () => {
      streaming = true;
      void sync();
    };
    source.onerror = () => (streaming = false);
    source.addEventListener("wizard.changed", () => void sync());
    const poll = setInterval(() => {
      if (!streaming && wizard.state?.current === "install_hub") void sync();
    }, POLL_MS);
    return () => {
      source.close();
      clearInterval(poll);
      target.id = "";
    };
  });
</script>

<svelte:window onfocus={refresh} />
<svelte:document onvisibilitychange={refresh} />

<svelte:head>
  <title>{t("app.title", { surface: t("onboarding.setup.title") })}</title>
</svelte:head>

<div class="setup">
  {#if wizard.state}
    <div class="side">
      <WizardProgress steps={wizard.state.steps} current={wizard.state.current} href={stepPath} />
    </div>
    <div class="content">
      {@render children()}
    </div>
  {:else if failed}
    <div class="content">
      <h1>{t("onboarding.setup.title")}</h1>
      <p role="alert">
        {t(failed === "offline" ? "onboarding.error.offline" : "onboarding.error.generic")}
      </p>
      <Button label="onboarding.account_setup.retry" onclick={load} />
    </div>
  {:else}
    <div class="content">
      <h1>{t("onboarding.setup.title")}</h1>
      <p role="status">{t("onboarding.setup.loading")}</p>
    </div>
  {/if}
  <p class="visually-hidden" aria-live="polite" data-testid="wizard-announcement">
    {wizard.announcement}
  </p>
</div>

<style>
  .setup {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .content {
    flex: 1;
    min-width: 0;
    max-width: 40rem;
  }

  .content :global(h1) {
    margin: var(--space-4) 0 var(--space-2);
    font-family: var(--type-web-display-family), system-ui, sans-serif;
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    line-height: var(--type-web-display-line-height);
    letter-spacing: var(--type-web-display-letter-spacing);
  }

  .content :global(h1:focus) {
    outline: none;
  }

  @media (min-width: 840px) {
    .setup {
      flex-direction: row;
      align-items: flex-start;
      gap: var(--space-6);
    }

    .side {
      flex: 0 0 256px;
      position: sticky;
      top: var(--space-4);
      padding-top: var(--space-4);
    }
  }
</style>
