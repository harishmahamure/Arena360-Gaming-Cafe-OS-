<script lang="ts">
  import { t } from "../messages/index.js";

  let {
    size = 64,
    class: className = "",
    ...rest
  }: {
    /** Width and height in CSS px. */
    size?: number;
    class?: string;
    [key: string]: unknown;
  } = $props();
</script>

<!-- The Arena 360 mark: a ring with a play triangle. Drawn from theme tokens only. -->
<svg
  class="brand-mark {className}"
  viewBox="0 0 48 48"
  width={size}
  height={size}
  role="img"
  aria-label={t("app.name")}
  data-component="brand-mark"
  {...rest}
>
  <circle cx="24" cy="24" r="21" fill="none" stroke="var(--accent)" stroke-width="4" />
  <path d="M19 15 35 24 19 33z" fill="var(--text)" />
</svg>

<style>
  .brand-mark {
    display: block;
    flex: none;
  }
</style>
