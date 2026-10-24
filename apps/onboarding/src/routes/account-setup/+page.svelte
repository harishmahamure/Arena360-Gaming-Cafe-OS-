<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { currentSession } from "../../lib/api";

  // Gated by `GET /v1/session` (refreshed once on a 401): without a session the owner goes
  // back to the start. A network failure or a server error offers a retry instead.
  let phase: "checking" | "signed_in" | "retry" = $state("checking");
  let offline = $state(false);

  async function check() {
    phase = "checking";
    const res = await currentSession();
    if (res.ok) {
      phase = "signed_in";
    } else if (res.offline || res.status >= 500) {
      offline = res.offline;
      phase = "retry";
    } else {
      await goto("/", { replaceState: true });
    }
  }

  onMount(check);
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("onboarding.account_setup.title") })}</title>
</svelte:head>

<h1>{t("onboarding.account_setup.title")}</h1>
{#if phase === "signed_in"}
  <p>{t("onboarding.account_setup.body")}</p>
{:else if phase === "retry"}
  <p role="alert">{t(offline ? "onboarding.error.offline" : "onboarding.error.generic")}</p>
  <Button label="onboarding.account_setup.retry" onclick={check} />
{:else}
  <p role="status">{t("onboarding.account_setup.checking")}</p>
{/if}

<style>
  h1 {
    margin: var(--space-4) 0 var(--space-2);
    font-family: var(--type-web-display-family), system-ui, sans-serif;
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    line-height: var(--type-web-display-line-height);
    letter-spacing: var(--type-web-display-letter-spacing);
  }
</style>
