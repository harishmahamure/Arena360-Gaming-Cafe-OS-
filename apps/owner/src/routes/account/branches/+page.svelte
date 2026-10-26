<script lang="ts">
  import { Button, TextField, TierLock, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { OwnerBranch } from "@arena/ui/generated";
  import { send, setupUrl } from "../../../lib/api";
  import BranchCard from "../../../lib/BranchCard.svelte";
  import { isFree, refresh, shell } from "../../../lib/shell.svelte";

  let name = $state("");
  let error: MessageKey | null = $state(null);
  let busy = $state(false);

  async function add(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = null;
    const res = await send<OwnerBranch>("POST", "/branches", { name });
    if (res.ok) {
      location.assign(setupUrl(res.data.id));
      return;
    }
    busy = false;
    error =
      res.code === "VALIDATION_FAILED"
        ? "owner.branches.error.name"
        : res.code === "FEATURE_NOT_ENTITLED"
          ? "owner.branches.error.locked"
          : "owner.branches.error.generic";
  }
</script>

<svelte:head><title>{t("app.title", { surface: t("owner.account.branches") })}</title></svelte:head>

<h1 class="type-web-display">{t("owner.account.branches")}</h1>
{#if shell.branches?.length}
  <ul class="list">
    {#each shell.branches as b (b.id)}<BranchCard branch={b} />{/each}
  </ul>
{:else if shell.ready}
  <p>{shell.branches ? t("owner.branches.empty") : t("owner.unavailable")}</p>
  {#if !shell.branches}<Button label="common.retry" variant="secondary" onclick={refresh} />{/if}
{/if}
{#if shell.branches?.length}
  {#if isFree()}
    <p class="type-hub-label">{t("owner.branches.add")}</p>
    <TierLock tier="Basic" />
    <p class="type-hub-label">{t("owner.branches.standby")}</p>
    <TierLock tier="Basic" />
  {:else}
    <form onsubmit={add}>
      <TextField label="owner.branches.add_name" bind:value={name} required />
      {#if error}<p role="alert">{t(error)}</p>{/if}
      <Button
        label="owner.branches.add"
        type="submit"
        disabled={busy}
        disabledReason="owner.loading"
      />
    </form>
  {/if}
{/if}
<p><a href="/account">{t("owner.account.back")}</a></p>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: 0;
    list-style: none;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 28rem;
  }
</style>
