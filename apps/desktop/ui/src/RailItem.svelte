<script>
  /* One rail row. Neutral until promoted, then it expands in place — the ones
   * below shift down. docs/05-UX-SPEC.md §3: that is the only layout change
   * permitted during a call, and slots never reorder. */
  let { item, expanded = false, editing = false, onactivate, onsave, oncancel } = $props();

  let draft = $state("");
  let draftTrack = $state("");

  $effect(() => {
    if (editing) {
      draft = item.response;
      draftTrack = item.talk_track.join("\n");
    }
  });

  const colour = $derived(`var(--${item.kind})`);

  function save() {
    onsave?.({
      objection_id: item.objection_id,
      response: draft,
      talk_track: draftTrack.split("\n").map((l) => l.trim()).filter(Boolean),
    });
  }
</script>

<div
  class="row"
  class:expanded
  style:--kind={colour}
  role="button"
  tabindex="0"
  onclick={() => onactivate?.(item)}
  onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onactivate?.(item)}
>
  <div class="head">
    <!-- The signature element. A monospace numeral in a hairline square that
         teaches the shortcut passively, and fills solid when promoted so the
         number stays legible and muscle memory holds. §2. -->
    <span class="badge" class:filled={expanded}>{item.slot}</span>
    <span class="label">{item.label}</span>
    {#if item.kind === "comply"}
      <span class="tag">COMPLY</span>
    {:else if item.kind === "disqualify"}
      <span class="tag">DISQUALIFY</span>
    {/if}
  </div>

  {#if expanded}
    <div class="body">
      {#if editing}
        <!-- 01-PRD.md §2: the target user will not author YAML, but will
             happily retype a response in their own words. -->
        <label class="field">
          <span>What to do</span>
          <textarea bind:value={draft} rows="3"></textarea>
        </label>
        <label class="field">
          <span>What to say — one line each</span>
          <textarea bind:value={draftTrack} rows="4"></textarea>
        </label>
        <div class="actions">
          <button class="primary" onclick={(e) => { e.stopPropagation(); save(); }}>Save</button>
          <button onclick={(e) => { e.stopPropagation(); oncancel?.(); }}>Cancel</button>
        </div>
      {:else}
        {#each item.response.split("\n").filter((l) => l.trim()) as line}
          <p class="direction">{line.trim()}</p>
        {/each}
        {#each item.talk_track as line}
          <p class="track">“{line}”</p>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .row {
    border-left: 3px solid transparent;
    padding: 9px 12px;
    border-radius: 4px;
  }
  .row.expanded {
    background: var(--surface-raised);
    border-left-color: var(--kind);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .badge {
    font-family: var(--mono);
    font-size: 12px;
    font-weight: 500;
    min-width: 22px;
    text-align: center;
    padding: 2px 0;
    border: 1px solid var(--edge);
    border-radius: 3px;
    color: var(--text-dim);
  }
  .badge.filled {
    background: var(--kind);
    border-color: var(--kind);
    color: var(--surface);
  }
  .label {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .tag {
    margin-left: auto;
    font-family: var(--mono);
    font-size: 10px;
    letter-spacing: 0.08em;
    color: var(--kind);
  }
  .body {
    margin: 10px 0 2px 32px;
  }
  .direction {
    margin: 0 0 8px;
    line-height: 1.45;
  }
  .track {
    margin: 0 0 6px;
    line-height: 1.5;
    font-style: italic;
    color: var(--text-dim);
  }
  .field {
    display: block;
    margin-bottom: 10px;
  }
  .field span {
    display: block;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-dim);
    margin-bottom: 4px;
  }
  textarea {
    width: 100%;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--edge);
    border-radius: 4px;
    padding: 7px;
    font: inherit;
    font-size: 14px;
    line-height: 1.45;
    resize: vertical;
    user-select: text;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .actions button {
    padding: 5px 12px;
    border: 1px solid var(--edge);
    border-radius: 4px;
    font-size: 13px;
    color: var(--text-dim);
  }
  .actions .primary {
    border-color: var(--kind);
    color: var(--kind);
  }
</style>
