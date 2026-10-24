<script lang="ts">
  import { goto } from "$app/navigation";
  import { stepPath, wizard } from "../../lib/wizard.svelte";

  $effect(() => {
    const current = wizard.state?.current;
    if (current) goto(stepPath(current), { replaceState: true });
  });
</script>
