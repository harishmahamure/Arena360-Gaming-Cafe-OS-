<script lang="ts">
  import { onMount } from "svelte";

  // Local time through `Intl` in the runtime's default locale (branch locale arrives in 1.12).
  const format = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

  let now = $state(new Date());

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
  >{format.format(now)}</time
>

<style>
  .clock {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
