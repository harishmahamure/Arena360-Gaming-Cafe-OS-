<script lang="ts">
  import { goto } from "$app/navigation";
  import { PUBLIC_OWNER_URL } from "$app/env/public";
  import { stepPath, wizard } from "../../lib/wizard.svelte";

  $effect(() => {
    const state = wizard.state;
    if (!state) return;
    if (state.steps.some((s) => s.key === "go_live" && s.state === "done")) {
      location.replace(PUBLIC_OWNER_URL.replace(/\/$/, "") + "/");
    } else goto(stepPath(state.current), { replaceState: true });
  });
</script>
