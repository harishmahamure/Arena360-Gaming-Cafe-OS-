<script lang="ts">
  import { formatTime } from "@arena/ui/format";
  import { onMount } from "svelte";
  import { branchLocale } from "./locale.svelte";

  // The time in the branch time zone (the runtime's own until the snapshot names one), in the
  // runtime's language. A tz change applies with the business day it takes effect on.
  let now = $state(new Date());
  const label = $derived(formatTime(now, branchLocale.current?.tz));

  onMount(() => {
    let timer: ReturnType<typeof setTimeout>;
    // Refresh on each minute boundary.
    const schedule = () => {
      const current = new Date();
      now = current;
      const wait = 60_000 - (current.getSeconds() * 1000 + current.getMilliseconds());
      timer = setTimeout(schedule, wait);
    };
    // A hidden window's timers are throttled: catch up when it is shown again.
    const onVisible = () => {
      if (document.visibilityState !== "visible") return;
      clearTimeout(timer);
      schedule();
    };
    schedule();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  });
</script>

<time class="clock type-hub-title" datetime={now.toISOString()} data-testid="hub-clock"
  >{label}</time
>

<style>
  .clock {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
