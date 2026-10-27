<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { PlayerHit, SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, convertSession, newIdempotencyKey, submitIntent } from "../auth";
  import CreateProfile from "./CreateProfile.svelte";
  import PlayerLookup from "./PlayerLookup.svelte";

  let {
    sessionId,
    rev,
    staff,
    busy = false,
    ondone,
  }: {
    sessionId: string;
    rev: number;
    staff: SessionStaff | null;
    busy?: boolean;
    ondone: (name: string) => void;
  } = $props();

  let mode: "closed" | "create" | "link" = $state("closed");
  let prefill = $state("+91");
  let refusal: MessageKey | null = $state(null);
  let working = $state(false);

  const close = () => {
    mode = "closed";
    refusal = null;
  };

  async function link(hit: PlayerHit) {
    if (working) return;
    working = true;
    refusal = null;
    const outcome = await submitIntent(
      { kind: "session.assign_player", session_id: sessionId, player_id: hit.player_id },
      newIdempotencyKey(),
      rev,
    ).finally(() => (working = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      mode = "closed";
      return ondone(hit.name);
    }
    const code = outcome.kind === "error" ? outcome.error.code : null;
    refusal =
      code === "PERMISSION_DENIED"
        ? "hub.refusal.permission"
        : code === "STALE" || code === "VALIDATION_FAILED"
          ? "hub.session.convert.refused"
          : "hub.settings.failed";
  }
</script>

<div class="convert" data-testid="convert-guest">
  {#if mode === "closed"}
    <div class="row">
      <Button
        label="hub.session.convert"
        variant="secondary"
        disabled={busy}
        disabledReason="hub.sheet.waiting"
        onclick={() => (mode = "create")}
      />
      <Button
        label="hub.session.link"
        variant="secondary"
        disabled={busy}
        disabledReason="hub.sheet.waiting"
        onclick={() => (mode = "link")}
      />
    </div>
  {:else if mode === "create"}
    <CreateProfile
      {staff}
      phone={prefill}
      save={(form, ticket) => convertSession(sessionId, rev, form, ticket)}
      elevationKind="session.assign_player"
      oncreated={(username) => {
        mode = "closed";
        ondone(username);
      }}
      oncancel={close}
    />
  {:else}
    <section class="link" aria-labelledby="link-title" data-testid="link-player">
      <h3 class="type-hub-title" id="link-title">{t("hub.session.link")}</h3>
      <PlayerLookup
        onpick={(hit) => void link(hit)}
        oncreate={(phone) => {
          prefill = phone;
          mode = "create";
        }}
      />
      {#if refusal}
        <p class="type-hub-label warn" role="alert">
          <span aria-hidden="true">⚠</span>
          {t(refusal)}
        </p>
      {/if}
      <div class="row">
        <Button label="hub.players.cancel" variant="secondary" onclick={close} />
      </div>
    </section>
  {/if}
</div>

<style>
  .convert,
  .link {
    display: grid;
    gap: var(--space-3);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  h3,
  p {
    margin: 0;
  }

  .warn {
    color: var(--text-field-error-text);
  }
</style>
