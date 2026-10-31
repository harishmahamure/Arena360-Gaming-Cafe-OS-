<script lang="ts">
  import { onMount } from "svelte";
  import { chooseMode, siteTheme, useSiteTheme } from "../site-theme.svelte.js";
  import ContrastSwitch from "./ContrastSwitch.svelte";
  import TabControl from "./TabControl.svelte";

  /**
   * The marketing footer's `theme-toggle`: System · Light · Dark plus a separate High contrast
   * switch. System follows `prefers-color-scheme` live; both choices are remembered per device.
   */
  onMount(useSiteTheme);

  const items = [
    { id: "system", label: "ui.theme.system" as const },
    { id: "light", label: "ui.theme.light" as const },
    { id: "dark", label: "ui.theme.dark" as const },
  ];

  function onchange(id: string) {
    chooseMode(id === "light" || id === "dark" ? id : "system");
  }
</script>

<div class="site-theme" data-component="theme-toggle">
  <TabControl {items} label="ui.theme.label" selected={siteTheme.mode} {onchange} />
  <ContrastSwitch />
</div>

<style>
  .site-theme {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
</style>
