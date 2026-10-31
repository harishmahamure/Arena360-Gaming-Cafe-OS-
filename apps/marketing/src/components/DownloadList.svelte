<script lang="ts">
  import { onMount } from "svelte";
  import { Button, LinkButton, t } from "@arena/ui";
  import {
    DOWNLOADS,
    fetchManifest,
    installerUrl,
    megabytes,
    playUrl,
    releaseDate,
    verifyCommand,
    type Release,
  } from "../lib/releases";

  /**
   * The installers, from the release manifest. The list read at build time is shown at once; the
   * browser reads the manifest again so a new release appears without a site redeploy. If that
   * read fails the build-time list stays, dated. A row without a complete manifest line says
   * "Coming soon" and has no link, so no link ever points to a file whose checksum is not shown.
   */
  let { releases, builtAt }: { releases: Release[]; builtAt: string } = $props();

  let live = $state<Release[] | null>(null);
  let stale = $state(false);
  let copied = $state<string | null>(null);

  const current = $derived(live ?? releases);
  const releaseOf = (id: string) => current.find((r) => r.product === id);

  onMount(async () => {
    try {
      live = await fetchManifest();
    } catch {
      stale = true;
    }
  });

  const shaId = (id: string) => `sha-${id}`; // i18n-ignore

  async function copy(sha: string, field: HTMLElement | null) {
    try {
      await navigator.clipboard.writeText(sha);
    } catch {
      (field as HTMLInputElement | null)?.select();
      document.execCommand("copy");
    }
    copied = sha;
  }
</script>

{#if stale}
  <p class="dl-stale" role="status">
    {t("site.downloads.stale", { date: releaseDate(builtAt) })}
  </p>
{/if}

<ul class="dl-list">
  {#each DOWNLOADS as item (item.id)}
    {@const release = item.kind === "file" ? releaseOf(item.id) : undefined}
    <li class="dl-row" data-product={item.id}>
      <div class="dl-head">
        <h3>{t(item.name)}</h3>
        <p>{t(item.about)}</p>
      </div>

      {#if item.kind === "play"}
        <div class="dl-body">
          <p>{t("site.downloads.play.note")}</p>
          <div class="dl-action">
            <LinkButton href={playUrl()} label="site.downloads.play.get" />
          </div>
        </div>
      {:else if release}
        <div class="dl-body">
          <dl class="dl-facts">
            <div>
              <dt>{t("site.downloads.version")}</dt>
              <dd>{release.version}</dd>
            </div>
            <div>
              <dt>{t("site.downloads.size")}</dt>
              <dd>{t("site.downloads.mb", { mb: megabytes(release.size) })}</dd>
            </div>
            <div>
              <dt>{t("site.downloads.released")}</dt>
              <dd>{releaseDate(release.published_at)}</dd>
            </div>
          </dl>

          <div class="dl-hash">
            <label for={shaId(item.id)}>{t("site.downloads.sha")}</label>
            <div class="dl-hash-row">
              <input
                id={shaId(item.id)}
                type="text"
                readonly
                spellcheck="false"
                value={release.sha256}
                onfocus={(event) => event.currentTarget.select()}
              />
              <Button
                variant="secondary"
                label={copied === release.sha256 ? "site.downloads.copied" : "site.downloads.copy"}
                onclick={() => copy(release.sha256, document.getElementById(shaId(item.id)))}
              />
            </div>
          </div>

          <div class="dl-action">
            <LinkButton
              href={installerUrl(release)}
              label="site.downloads.download"
              params={{ name: t(item.name) }}
            />
          </div>

          <details class="dl-verify">
            <summary>{t("site.downloads.verify")}</summary>
            <p>{t("site.downloads.verify.steps")}</p>
            <pre tabindex="0"><code>{verifyCommand(release)}</code></pre>
            <p>{t("site.downloads.verify.compare")}</p>
          </details>
        </div>
      {:else}
        <p class="dl-soon">{t("site.downloads.soon")}</p>
      {/if}
    </li>
  {/each}
</ul>

<p class="visually-hidden" role="status" aria-live="polite">
  {copied ? t("site.downloads.copied.status") : ""}
</p>
