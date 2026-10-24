<script lang="ts">
  import { onMount } from "svelte";
  import TabControl from "./TabControl.svelte";
  import { readThemeChoice, resolveTheme, storeThemeChoice, type ThemeChoice } from "../theme.js";

  /**
   * System · Light · Dark for the web surfaces (never the branch apps). System is the default
   * and follows `prefers-color-scheme` live. The choice is remembered per device and sets
   * `data-theme` on `<html>`.
   */
  let choice: ThemeChoice = $state("system");
  let prefersDark = $state(false);

  const items = [
    { id: "system", label: "ui.theme.system" as const },
    { id: "light", label: "ui.theme.light" as const },
    { id: "dark", label: "ui.theme.dark" as const },
  ];

  function apply() {
    document.documentElement.dataset.theme = resolveTheme(choice, prefersDark);
  }

  function onchange(id: string) {
    choice = id === "light" || id === "dark" ? id : "system";
    storeThemeChoice(choice);
    apply();
  }

  onMount(() => {
    choice = readThemeChoice();
    const query = window.matchMedia?.("(prefers-color-scheme: dark)");
    prefersDark = query?.matches ?? false;
    apply();
    const follow = (event: MediaQueryListEvent) => {
      prefersDark = event.matches;
      apply();
    };
    query?.addEventListener("change", follow);
    return () => query?.removeEventListener("change", follow);
  });
</script>

<div class="theme-toggle" data-component="theme-toggle">
  <TabControl {items} label="ui.theme.label" selected={choice} {onchange} />
</div>
