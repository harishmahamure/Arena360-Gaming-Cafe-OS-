<script lang="ts">
  // Story 10.5: every "Upgrade" link and "Add branch" lands here and reuses the Story 10.4 picker
  // and Checkout for one branch. `?branch=` names it (without it the Free branches are offered),
  // `?tier=` is what the lock needed, `?return=` the page to go back to.
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { OwnerBranch } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { PUBLIC_OWNER_URL } from "$app/env/public";
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { getSubscriptionStatus, getWizard, listBranches } from "../../lib/api";
  import TierStep from "../../lib/TierStep.svelte";
  import { hasSubscription, lockedTier, returnTarget, subscriptionUrl } from "../../lib/upgrade";
  import { clearWizard, setWizard, target } from "../../lib/wizard.svelte";

  type View = "loading" | "choose" | "pick" | "done" | "failed";

  const tier = lockedTier(page.url.searchParams.get("tier"));
  let view: View = $state("loading");
  let failure: MessageKey = $state("onboarding.error.generic");
  let candidates: OwnerBranch[] = $state([]);
  let branchId = $state("");
  let backTo = "";

  function leave(to: string) {
    location.assign(to);
  }

  function fail(key: MessageKey) {
    failure = key;
    view = "failed";
  }

  async function open(id: string) {
    view = "loading";
    branchId = id;
    target.id = id;
    clearWizard();
    const wizard = await getWizard();
    if (!wizard.ok) {
      if (wizard.error.code === "SETUP_INCOMPLETE") {
        await goto("/account-setup", { replaceState: true });
      } else if (wizard.error.code === "FORBIDDEN") fail("onboarding.upgrade.forbidden");
      else fail(wizard.offline ? "onboarding.error.offline" : "onboarding.error.generic");
      return;
    }
    // A branch that already has a subscription is managed, not bought again.
    const status = await getSubscriptionStatus(id);
    if (status.ok && hasSubscription(status.data.state)) {
      leave(subscriptionUrl(PUBLIC_OWNER_URL, id));
      return;
    }
    if (!status.ok && status.status !== 404) {
      fail(status.offline ? "onboarding.error.offline" : "onboarding.error.generic");
      return;
    }
    setWizard(wizard.data);
    view = "pick";
  }

  async function start() {
    const named = page.url.searchParams.get("branch");
    if (named) return open(named);
    const res = await listBranches();
    if (!res.ok) {
      if (res.error.code === "SETUP_INCOMPLETE") {
        await goto("/account-setup", { replaceState: true });
      } else fail(res.offline ? "onboarding.error.offline" : "onboarding.error.generic");
      return;
    }
    candidates = res.data.filter((b) => b.tier === "free");
    if (candidates.length === 1) return open(candidates[0]!.id);
    if (candidates.length > 1) view = "choose";
    else if (res.data.length > 0) leave(subscriptionUrl(PUBLIC_OWNER_URL, res.data[0]!.id));
    else fail("onboarding.upgrade.none");
  }

  onMount(() => {
    backTo = returnTarget(page.url.searchParams.get("return"), PUBLIC_OWNER_URL, location.origin);
    void start();
    return () => {
      target.id = "";
      clearWizard();
    };
  });

  const upgrade = $derived({
    tier,
    onDone: () => {
      view = "done";
      leave(backTo);
    },
    onExists: () => leave(subscriptionUrl(PUBLIC_OWNER_URL, branchId)),
    onBack: () => leave(backTo),
  });
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("onboarding.upgrade.title") })}</title>
</svelte:head>

{#if view === "pick"}
  <div class="upgrade">
    <TierStep {upgrade} />
  </div>
{:else if view === "choose"}
  <h1 tabindex="-1">{t("onboarding.upgrade.title")}</h1>
  <p>{t("onboarding.upgrade.pick_branch")}</p>
  <ul class="branches">
    {#each candidates as b (b.id)}
      <li>
        <button class="pick" type="button" onclick={() => open(b.id)}>
          {t("onboarding.upgrade.pick_button", { branch: b.name })}
        </button>
      </li>
    {/each}
  </ul>
{:else if view === "failed"}
  <h1 tabindex="-1">{t("onboarding.upgrade.title")}</h1>
  <p role="alert">{t(failure)}</p>
  <Button label="onboarding.upgrade.back" variant="secondary" onclick={() => leave(backTo)} />
{:else if view === "done"}
  <h1 tabindex="-1">{t("onboarding.upgrade.title")}</h1>
  <p role="status">{t("onboarding.upgrade.done")}</p>
{:else}
  <h1 tabindex="-1">{t("onboarding.upgrade.title")}</h1>
  <p role="status">{t("onboarding.setup.loading")}</p>
{/if}

<style>
  .upgrade {
    max-width: 40rem;
  }

  :global(main h1) {
    margin: var(--space-4) 0 var(--space-2);
    font-family: var(--type-web-display-family), system-ui, sans-serif;
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    line-height: var(--type-web-display-line-height);
  }

  .pick {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--card-background);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }

  .branches {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: 0;
    list-style: none;
  }
</style>
