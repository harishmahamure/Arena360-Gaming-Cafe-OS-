<script lang="ts">
  import { InlinePin, UndoToast, formatMoney } from "@arena/ui";
  import type { SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { branchLocale } from "./locale.svelte";
  import { approveUndo, cancelApproval, runUndo, secondsLeft, undo } from "./undo.svelte";

  let { staff }: { staff: SessionStaff | null } = $props();

  const MESSAGE: Record<"end" | "forced" | "settle" | "topup" | "collect" | "plan", MessageKey> = {
    end: "hub.undo.ended",
    forced: "hub.undo.forced",
    settle: "hub.undo.settled",
    topup: "hub.undo.topped_up",
    collect: "hub.undo.collected",
    plan: "hub.undo.plan_sold",
  };
</script>

{#if undo.current && branchLocale.current}
  {@const u = undo.current}
  <div class="undo-dock">
    <UndoToast
      message={MESSAGE[u.kind]}
      params={{ amount: formatMoney(u.amountMinor, branchLocale.current) }}
      seconds={secondsLeft(u)}
      busy={u.phase === "sending"}
      notice={u.notice}
      refused={u.refused}
      error={u.error}
      onundo={() => void runUndo(staff)}
    >
      {#if u.phase === "approving"}
        <InlinePin
          message="ui.pin.prompt"
          onsubmit={(pin) => approveUndo(staff, pin)}
          oncancel={cancelApproval}
        />
      {/if}
    </UndoToast>
  </div>
{/if}

<style>
  .undo-dock {
    position: fixed;
    inset: auto 0 var(--space-4);
    z-index: 12;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }

  @media (max-width: 839px) {
    .undo-dock {
      bottom: calc(var(--space-9) + var(--space-3));
    }
  }
</style>
