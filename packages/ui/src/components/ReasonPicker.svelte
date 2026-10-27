<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t, type MessageKey } from "../messages/index.js";
  import Button from "./Button.svelte";
  import TextField from "./TextField.svelte";

  type Option = { value: string; label: MessageKey };

  /**
   * Picks the reason an intent requires, in place (it follows an accepted PIN). Radio options
   * from catalogue keys, then Confirm. Confirming with nothing chosen shows the field error and
   * keeps focus on the options. `onconfirm` may answer a catalogue key, shown inline (a throw
   * shows `ui.pin.unreachable`). Esc calls `oncancel` and returns focus to `returnFocusTo`
   * (not while a confirm is in flight). With `noteFor`, choosing that option reveals a required
   * note field and `onconfirm` gets the trimmed note as its second argument.
   */
  let {
    options,
    onconfirm,
    oncancel,
    legend = "ui.reason.legend",
    returnFocusTo = null,
    autofocus = true,
    noteFor = undefined,
    noteLabel = "ui.reason.note",
  }: {
    options: Option[];
    onconfirm: (value: string, note?: string) => void | MessageKey | Promise<void | MessageKey>;
    oncancel: () => void;
    legend?: MessageKey;
    returnFocusTo?: HTMLElement | null;
    autofocus?: boolean;
    noteFor?: string;
    noteLabel?: MessageKey;
  } = $props();

  const id = $props.id();
  const errorId = id + "-error";

  let chosen = $state("");
  let note = $state("");
  const wantsNote = $derived(noteFor !== undefined && chosen === noteFor);
  let error: MessageKey | null = $state(null);
  let busy = $state(false);
  let group: HTMLFieldSetElement | undefined = $state();

  function firstRadio(): HTMLInputElement | null {
    // The chosen option if there is one, else the first.
    return (
      group?.querySelector<HTMLInputElement>("input:checked") ?? // i18n-ignore (a selector)
      group?.querySelector<HTMLInputElement>("input") ?? // i18n-ignore (a selector)
      null
    );
  }

  async function confirm() {
    if (busy) return;
    if (!chosen) {
      error = "ui.reason.required";
      await tick();
      firstRadio()?.focus();
      return;
    }
    const body = note.trim();
    if (wantsNote && (!body || [...body].length > 140)) {
      error = "ui.reason.note_required";
      await tick();
      group?.parentElement?.querySelector<HTMLInputElement>("input[type=text]")?.focus(); // i18n-ignore (a selector)
      return;
    }
    busy = true;
    let refusal: MessageKey | void;
    try {
      refusal = await (wantsNote ? onconfirm(chosen, body) : onconfirm(chosen));
    } catch {
      refusal = "ui.pin.unreachable";
    } finally {
      busy = false;
    }
    if (refusal) {
      error = refusal;
      await tick();
      firstRadio()?.focus();
    }
  }

  function cancel() {
    if (busy) return;
    chosen = "";
    note = "";
    error = null;
    oncancel();
    returnFocusTo?.focus();
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    } else if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
      // Enter on a radio confirms instead of submitting a surrounding form.
      event.preventDefault();
      void confirm();
    }
  }

  onMount(() => {
    if (autofocus) firstRadio()?.focus();
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="reason-picker" data-component="reason-picker" {onkeydown}>
  <fieldset class="group" bind:this={group} aria-describedby={error ? errorId : undefined}>
    <legend class="legend">{t(legend)}</legend>
    {#each options as option (option.value)}
      <label class="option">
        <input
          type="radio"
          name="{id}-reason"
          value={option.value}
          bind:group={chosen}
          onchange={() => (error = null)}
        />
        <span>{t(option.label)}</span>
      </label>
    {/each}
  </fieldset>
  {#if wantsNote}
    <TextField label={noteLabel} bind:value={note} />
  {/if}
  <p class="error" id={errorId} aria-live="polite">
    {#if error}<span aria-hidden="true">⚠</span>
      <span>{t(error)}</span>{/if}
  </p>
  <div class="actions">
    <Button label="ui.reason.confirm" onclick={() => void confirm()} />
    <Button label="ui.pin.cancel" variant="secondary" onclick={cancel} />
  </div>
</div>

<style>
  .reason-picker {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 20rem;
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--bg);
    color: var(--text);
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }

  .legend {
    padding: 0;
    margin-bottom: var(--space-2);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .option {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
    cursor: pointer;
  }

  .option input {
    width: var(--space-touch-min);
    height: var(--space-touch-min);
    margin: 0;
    accent-color: var(--accent);
  }

  .error {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    margin: 0;
    color: var(--text-field-error-text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
