<script>
  /* A comply card does not expand like the others — it takes the whole panel
   * and hides the rail underneath, because the correct number of alternatives
   * to offer at this moment is zero. docs/05-UX-SPEC.md §3, 06-COMPLIANCE.md §3.
   *
   * No feedback buttons: this card is not a suggestion to be rated. */
  let { item, onlog, ondismiss } = $props();
  let logged = $state(false);

  function log() {
    onlog?.(item.objection_id);
    logged = true;
  }
</script>

<div class="panel">
  <div class="head">
    <span class="badge">{item.slot}</span>
    <h1>REMOVAL REQUESTED</h1>
  </div>

  {#each item.response.split("\n").filter((l) => l.trim()) as line}
    <p class="direction">{line.trim()}</p>
  {/each}

  {#each item.talk_track as line}
    <p class="track">“{line}”</p>
  {/each}

  <div class="spacer"></div>

  {#if logged}
    <p class="done">Logged. Enter the removal in your dialer.</p>
  {:else}
    <button class="primary" onclick={log}>Log removal and end call</button>
  {/if}

  <!-- 06-COMPLIANCE.md §6: claiming more than this would invite reliance on a
       system that cannot deliver, which is worse than claiming nothing. -->
  <p class="fineprint">
    Riposte does not maintain a do-not-call list and does not stop future
    dialing. This log is so you can enter the removal in your dialer and CRM.
  </p>

  <button class="dismiss" onclick={() => ondismiss?.()}>Dismiss</button>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 18px 20px 14px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }
  .badge {
    font-family: var(--mono);
    font-size: 13px;
    padding: 3px 8px;
    background: var(--comply);
    color: var(--surface);
    border-radius: 3px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
    letter-spacing: 0.02em;
    /* Colour is never the only signal — the words say it too. §8. */
    color: var(--comply);
  }
  .direction {
    margin: 0 0 10px;
    line-height: 1.45;
    font-size: 15px;
  }
  .track {
    margin: 0 0 8px;
    line-height: 1.5;
    font-style: italic;
    color: var(--text-dim);
  }
  .spacer { flex: 1; min-height: 12px; }
  .primary {
    width: 100%;
    padding: 11px;
    background: var(--comply);
    color: var(--surface);
    border-radius: 4px;
    font-weight: 600;
    font-size: 15px;
  }
  .done {
    margin: 0;
    padding: 11px;
    text-align: center;
    border: 1px solid var(--edge);
    border-radius: 4px;
    color: var(--live);
    font-size: 14px;
  }
  .fineprint {
    margin: 10px 0 0;
    font-size: 11px;
    line-height: 1.4;
    color: var(--text-dim);
  }
  .dismiss {
    margin-top: 10px;
    align-self: center;
    font-size: 12px;
    color: var(--text-dim);
  }
</style>
