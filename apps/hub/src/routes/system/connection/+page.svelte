<script lang="ts">
  import { StatusBadge, t } from "@arena/ui";
  import type { TunnelLine } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { TUNNEL_LABELS, loadTunnel, needsAttention } from "../../../lib/tunnel";

  // Remote access: the Cloudflare Tunnel the Windows Hub runs when the branch's plan has it. The
  // line is read from the Hub and re-read while the page is open; the state is always written out.

  const POLL_MS = 10_000;

  let line: TunnelLine | null = $state(null);
  let failed = $state(false);

  async function refresh() {
    const next = await loadTunnel();
    failed = next === null;
    line = next;
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });
</script>

<section class="connection" aria-labelledby="connection-title">
  <h2 class="type-hub-title" id="connection-title">{t("hub.tunnel.title")}</h2>
  <p class="type-hub-body">{t("hub.tunnel.intro")}</p>

  <div class="row" role="status" data-testid="tunnel-line">
    {#if line}
      <StatusBadge
        tone={line.state === "on" ? "ok" : needsAttention(line.state) ? "danger" : "muted"}
        label={TUNNEL_LABELS[line.state]}
      />
      {#if line.hostname && line.state !== "off" && line.state !== "unsupported_platform"}
        <p class="type-hub-body" data-testid="tunnel-hostname">
          {t("hub.tunnel.address", { hostname: line.hostname })}
        </p>
      {/if}
    {:else if failed}
      <p class="type-hub-body" data-testid="tunnel-unreadable">{t("hub.tunnel.unreadable")}</p>
    {/if}
  </div>
</section>

<style>
  .connection {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  h2,
  p {
    margin: 0;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
</style>
