<script lang="ts">
  import { Button, formatMoney, t } from "@arena/ui";
  import type { PlayerHit } from "@arena/ui/generated";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, findPlayers } from "../auth";
  import { branchLocale } from "../locale.svelte";

  let {
    onpick,
    onguest,
    oncreate,
  }: {
    onpick: (hit: PlayerHit) => void;
    onguest?: () => void;
    oncreate?: (phone: string) => void;
  } = $props();

  let q = $state("");
  let hits: PlayerHit[] = $state([]);
  let answered = $state("");
  let failed = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let latest = 0;

  const digits = $derived(q.replace(/\D/g, ""));
  const none = $derived(digits.length >= 10 && answered === digits && !hits.length && !failed);

  export const focus = () => input?.focus();

  export async function search() {
    const asked = digits;
    const turn = ++latest;
    if (!asked) {
      hits = [];
      answered = "";
      failed = false;
      return;
    }
    const outcome = await findPlayers(asked);
    if (turn !== latest) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    failed = outcome.kind !== "ok";
    hits = outcome.kind === "ok" ? outcome.value : [];
    answered = asked;
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && hits.length && answered === digits) {
      event.preventDefault();
      onpick(hits[0]);
    }
  }
</script>

<div class="lookup" data-testid="player-lookup">
  <div class="field">
    <label class="type-hub-label" for="player-lookup-q">{t("hub.players.lookup.label")}</label>
    <div class="row">
      <input
        id="player-lookup-q"
        class="ui-interactive control"
        type="text"
        inputmode="tel"
        autocomplete="off"
        data-player-lookup
        bind:this={input}
        bind:value={q}
        oninput={() => void search()}
        {onkeydown}
      />
      {#if onguest}
        <Button label="hub.players.lookup.guest" variant="secondary" onclick={onguest} />
      {/if}
    </div>
  </div>
  <div role="status">
    {#if failed}
      <p class="warn type-hub-label">
        <span aria-hidden="true">⚠</span>
        {t("hub.players.lookup.failed")}
      </p>
    {:else if none}
      <p class="type-hub-label">{t("hub.players.lookup.none")}</p>
      <div class="row">
        {#if oncreate}
          <Button
            label="hub.players.create"
            onclick={() => oncreate(digits.length === 10 ? `+91${digits}` : `+${digits}`)}
          />
        {/if}
        {#if onguest}
          <Button label="hub.players.lookup.guest" variant="secondary" onclick={onguest} />
        {/if}
      </div>
    {/if}
  </div>
  {#if hits.length}
    <ul class="hits" data-testid="player-hits">
      {#each hits as hit (hit.player_id)}
        <li>
          <button
            type="button"
            class="ui-interactive hit type-hub-body"
            onclick={() => onpick(hit)}
          >
            {#if hit.outstanding_minor > 0 && branchLocale.current}
              <span class="owes tabular-nums" data-testid="hit-owes">
                <span aria-hidden="true">⚠</span>
                {t("hub.players.lookup.owes", {
                  amount: formatMoney(hit.outstanding_minor, branchLocale.current),
                })}
              </span>
            {/if}
            <span>{hit.name}</span>
            <span class="phone">{hit.phone}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .lookup,
  .field {
    display: grid;
    gap: var(--space-2);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .control {
    flex: 1 1 12em;
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
    font: inherit;
  }

  p {
    margin: 0;
  }

  .warn {
    color: var(--text-field-error-text);
  }

  .hits {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .hit {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
    inline-size: 100%;
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
    font: inherit;
    text-align: start;
    cursor: pointer;
  }

  .phone {
    font-variant-numeric: tabular-nums;
  }

  .owes {
    color: var(--warning-text);
    font-weight: 600;
  }
</style>
