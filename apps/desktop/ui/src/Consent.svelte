<script>
  /* docs/05-UX-SPEC.md §7 screen 2, and 06-COMPLIANCE.md §1. Not skippable,
   * not defaulted checked. Screen 3 (audio setup) is absent because there is
   * no audio yet — showing a level meter that cannot move would be worse than
   * omitting the step. */
  let { onaccept } = $props();
  let acknowledged = $state(false);
</script>

<div class="screen">
  <h1>Before your first call</h1>

  <p>
    Riposte keeps your objection responses one keystroke away. Right now it does
    exactly that and nothing else — it does not listen to your calls, transcribe
    them, or send anything anywhere.
  </p>

  <div class="notice">
    <h2>When call listening arrives</h2>
    <p>
      A later version will listen to call audio to recognise objections as they
      happen. In roughly a dozen US states, recording a call without every
      party's consent is a crime, not a policy violation — and several are large
      insurance markets.
    </p>
    <p>A disclosure you can read at the start of a call:</p>
    <blockquote>
      “Just so you know, I use a tool that takes notes during our call — is that
      alright with you?”
    </blockquote>
    <p class="dim">
      Riposte is not a TCPA compliance system. It does not scrub lists, honour
      do-not-call registries, or prevent a single non-compliant dial. Have your
      own counsel review your disclosure and DNC process.
    </p>
  </div>

  <label class="ack">
    <input type="checkbox" bind:checked={acknowledged} />
    <span>I understand, and I am responsible for my own recording disclosure.</span>
  </label>

  <button class="primary" disabled={!acknowledged} onclick={() => onaccept?.()}>
    Continue
  </button>
</div>

<style>
  .screen {
    height: 100%;
    overflow-y: auto;
    padding: 20px;
  }
  h1 { margin: 0 0 12px; font-size: 19px; font-weight: 600; }
  h2 { margin: 0 0 8px; font-size: 13px; font-weight: 600; color: var(--comply); }
  p { margin: 0 0 10px; line-height: 1.5; font-size: 14px; }
  .notice {
    border: 1px solid var(--edge);
    border-left: 3px solid var(--comply);
    border-radius: 4px;
    padding: 12px;
    margin: 14px 0;
  }
  blockquote {
    margin: 0 0 10px;
    padding-left: 10px;
    border-left: 2px solid var(--edge);
    font-style: italic;
    color: var(--text-dim);
    line-height: 1.5;
    font-size: 14px;
  }
  .dim { color: var(--text-dim); font-size: 12px; }
  .ack {
    display: flex;
    gap: 9px;
    align-items: flex-start;
    margin: 14px 0;
    font-size: 13px;
    line-height: 1.45;
    cursor: pointer;
  }
  .ack input { margin-top: 2px; }
  .primary {
    width: 100%;
    padding: 10px;
    border-radius: 4px;
    background: var(--rebut);
    color: var(--surface);
    font-weight: 600;
  }
  .primary:disabled {
    background: var(--surface-raised);
    color: var(--text-dim);
    cursor: not-allowed;
  }
</style>
