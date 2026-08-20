<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import RailItem from "./RailItem.svelte";
  import ComplyCard from "./ComplyCard.svelte";
  import Consent from "./Consent.svelte";

  let rail = $state({ items: [], below_fold: [], playbook_name: "" });
  let caps = $state({ capture_exclusion: false, platform: "" });
  let stats = $state({ manual: 0, detected: 0 });

  let expandedId = $state(null);
  let editingId = $state(null);
  let complyCard = $state(null);
  let showConsent = $state(false);
  let practice = $state(false);
  let showMore = $state(false);
  let excludeCapture = $state(true);
  let excludeApplied = $state(false);
  let error = $state(null);

  const expanded = $derived(rail.items.find((i) => i.objection_id === expandedId) ?? null);

  async function refresh() {
    try {
      rail = await invoke("get_rail");
      stats = await invoke("get_stats");
    } catch (e) {
      // Layer 0 must not die on a bad read — show it and keep the shell up.
      error = String(e);
    }
  }

  async function activate(item, origin) {
    try {
      const card =
        practice
          ? await invoke("practice_card", { objectionId: item.objection_id })
          : await invoke("trigger_card", { objectionId: item.objection_id, origin });

      // A comply card takes the whole panel and hides the rail. §3.
      if (card.kind === "comply") {
        complyCard = card;
        expandedId = null;
      } else {
        // One expanded item at a time — two open cards is more than can be
        // read while talking.
        expandedId = card.objection_id;
        editingId = null;
      }
      if (!practice) stats = await invoke("get_stats");
    } catch (e) {
      error = String(e);
    }
  }

  async function save(edit) {
    try {
      await invoke("save_card", { edit });
      editingId = null;
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleCapture() {
    excludeCapture = !excludeCapture;
    try {
      excludeApplied = await invoke("set_capture_exclusion", { enabled: excludeCapture });
    } catch (e) {
      error = String(e);
    }
  }

  function onKey(e) {
    if (e.key === "Escape") {
      if (complyCard) complyCard = null;
      else if (editingId) editingId = null;
      else expandedId = null;
      return;
    }
    // In-window digits are a convenience while the overlay has focus. The
    // global binding is Ctrl+Alt+<n> — see docs/DECISIONS.md D18.
    if (e.target.tagName === "TEXTAREA" || e.target.tagName === "INPUT") return;
    const n = Number(e.key);
    if (n >= 1 && n <= 6) {
      const item = rail.items.find((i) => i.slot === n);
      if (item) activate(item, "manual_hotkey");
    }
  }

  $effect(() => {
    (async () => {
      caps = await invoke("get_capabilities");
      const seen = await invoke("get_setting", { key: "onboarding.consent_ack" });
      showConsent = seen !== "1";
      const ex = await invoke("get_setting", { key: "overlay.exclude_from_capture" });
      excludeCapture = ex === null || ex === "1";
      excludeApplied = caps.capture_exclusion && excludeCapture;
      await refresh();
    })();

    const un = listen("rail:hotkey", (e) => {
      const item = rail.items.find((i) => i.slot === e.payload);
      if (item) activate(item, "manual_hotkey");
    });
    return () => un.then((f) => f());
  });

  async function acceptConsent() {
    await invoke("set_setting", { key: "onboarding.consent_ack", value: "1" });
    showConsent = false;
  }
</script>

<svelte:window on:keydown={onKey} />

{#if showConsent}
  <Consent onaccept={acceptConsent} />
{:else if complyCard}
  <ComplyCard
    item={complyCard}
    onlog={(id) => invoke("log_removal", { objectionId: id })}
    ondismiss={() => (complyCard = null)}
  />
{:else}
  <div class="shell">
    <!-- Status bar. Doubles as the drag handle: the window is borderless. -->
    <div class="statusbar" data-tauri-drag-region>
      <span class="dot" class:practice></span>
      <span class="state">{practice ? "PRACTICE" : "READY"}</span>
      <span class="spacer"></span>
      <button
        class="chip"
        class:on={practice}
        title="Drill the hotkeys between calls. Practice presses are not counted."
        onclick={() => (practice = !practice)}>practice</button
      >
      <button class="chip" title="Minimise" onclick={() => getCurrentWindow().minimize()}>—</button>
      <button class="chip" title="Close" onclick={() => getCurrentWindow().close()}>✕</button>
    </div>

    {#if error}
      <div class="error">{error}<button onclick={() => (error = null)}>dismiss</button></div>
    {/if}

    <div class="rail">
      {#each rail.items as item (item.objection_id)}
        <RailItem
          {item}
          expanded={item.objection_id === expandedId}
          editing={item.objection_id === editingId}
          onactivate={(i) => activate(i, "manual_click")}
          onsave={save}
          oncancel={() => (editingId = null)}
        />
        {#if item.objection_id === expandedId && editingId !== item.objection_id}
          <button class="editlink" onclick={() => (editingId = item.objection_id)}>
            this isn’t how I say it — edit
          </button>
        {/if}
      {/each}
    </div>

    {#if showMore}
      <div class="more">
        {#each rail.below_fold as item (item.objection_id)}
          <button class="morerow" onclick={() => activate(item, "manual_click")}>
            <span style:color={`var(--${item.kind})`}>·</span> {item.label}
          </button>
        {/each}
      </div>
    {/if}

    <div class="footer">
      <button class="chip" onclick={() => (showMore = !showMore)}>
        {showMore ? "⌃" : "⌄"} more ({rail.below_fold.length})
      </button>
      <span class="spacer"></span>
      <button
        class="chip"
        class:on={excludeApplied}
        disabled={!caps.capture_exclusion}
        title={caps.capture_exclusion
          ? "Hide this window from Zoom, Teams, OBS and Snipping Tool. Not a security feature — it will not stop a photograph of the screen."
          : `Not available on ${caps.platform} — Windows 10 2004+ only`}
        onclick={toggleCapture}
      >
        {excludeApplied ? "hidden from share" : "visible in share"}
      </button>
      <span class="usage" title="Rail uses recorded — docs/01-PRD.md §7">
        {stats.manual}
      </span>
    </div>
  </div>
{/if}

<style>
  .shell { display: flex; flex-direction: column; height: 100%; }
  .statusbar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 10px;
    border-bottom: 1px solid var(--edge);
    flex-shrink: 0;
  }
  .dot {
    width: 8px; height: 8px; border-radius: 50%;
    background: var(--text-dim);
  }
  .dot.practice { background: var(--rebut); }
  .state {
    font-family: var(--mono);
    font-size: 11px;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }
  .spacer { flex: 1; }
  .chip {
    font-size: 11px;
    color: var(--text-dim);
    padding: 3px 7px;
    border-radius: 3px;
    border: 1px solid transparent;
  }
  .chip:hover:not(:disabled) { border-color: var(--edge); }
  .chip.on { color: var(--live); border-color: var(--edge); }
  .chip:disabled { opacity: 0.4; cursor: not-allowed; }
  .rail { padding: 8px 6px; overflow-y: auto; flex: 1; }
  .editlink {
    display: block;
    margin: 0 0 6px 44px;
    font-size: 11px;
    color: var(--text-dim);
    text-decoration: underline;
  }
  .more {
    border-top: 1px solid var(--edge);
    padding: 6px;
    max-height: 160px;
    overflow-y: auto;
  }
  .morerow {
    display: block;
    width: 100%;
    text-align: left;
    padding: 6px 10px;
    font-size: 14px;
    border-radius: 4px;
  }
  .morerow:hover { background: var(--surface-raised); }
  .footer {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 8px;
    border-top: 1px solid var(--edge);
    flex-shrink: 0;
  }
  .usage {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text-dim);
  }
  .error {
    background: var(--surface-raised);
    border-left: 3px solid var(--comply);
    padding: 8px 10px;
    font-size: 12px;
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .error button { font-size: 11px; color: var(--text-dim); text-decoration: underline; }
</style>
