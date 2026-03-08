<script>
  import { updateCameraConfig } from '../lib/api.js';

  let { camera = '', config = {}, onUpdate = () => {} } = $props();

  let expanded = $state(false);
  let saving = $state(false);
  let uppX = $state(0);
  let uppY = $state(0);
  let flipX = $state(false);
  let flipY = $state(false);

  // Sync local state when props change
  $effect(() => {
    if (config) {
      uppX = config.upp_x || 0;
      uppY = config.upp_y || 0;
      flipX = config.flip_x || false;
      flipY = config.flip_y || false;
    }
  });

  function fov(upp, pixels) {
    return (upp * pixels).toFixed(1);
  }

  async function save() {
    saving = true;
    try {
      await updateCameraConfig(camera, {
        upp_x: uppX,
        upp_y: uppY,
        flip_x: flipX,
        flip_y: flipY,
      });
      onUpdate();
    } catch (e) {
      console.error('Failed to save camera config:', e);
    }
    saving = false;
  }

  let dirty = $derived(
    config && (
      uppX !== config.upp_x ||
      uppY !== config.upp_y ||
      flipX !== config.flip_x ||
      flipY !== config.flip_y
    )
  );
</script>

<div class="calibration">
  <button class="cal-toggle" onclick={() => expanded = !expanded}>
    {expanded ? '▾' : '▸'} Calibration
    <span class="cal-summary">
      {config ? `${config.upp_x?.toFixed(4)} × ${config.upp_y?.toFixed(4)} mm/px` : ''}
    </span>
  </button>

  {#if expanded}
    <div class="cal-panel">
      <div class="cal-row">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label>UPP X <span class="unit">mm/px</span></label>
        <input type="number" step="0.0001" bind:value={uppX} />
      </div>
      <div class="cal-row">
        <!-- svelte-ignore a11y_label_has_associated_control -->
        <label>UPP Y <span class="unit">mm/px</span></label>
        <input type="number" step="0.0001" bind:value={uppY} />
      </div>
      <div class="cal-row">
        <span class="field-label">Flip X</span>
        <button
          class="flip-toggle"
          class:active={flipX}
          onclick={() => flipX = !flipX}
        >{flipX ? 'ON' : 'OFF'}</button>
      </div>
      <div class="cal-row">
        <span class="field-label">Flip Y</span>
        <button
          class="flip-toggle"
          class:active={flipY}
          onclick={() => flipY = !flipY}
        >{flipY ? 'ON' : 'OFF'}</button>
      </div>

      {#if config}
        <div class="cal-fov">
          FOV: {fov(uppX, config.width)} × {fov(uppY, config.height)} mm
        </div>
      {/if}

      <button
        class="cal-save"
        onclick={save}
        disabled={saving || !dirty}
      >
        {saving ? 'Saving...' : 'Save'}
      </button>
    </div>
  {/if}
</div>

<style>
  .calibration {
    background: #16213e;
    border-top: 1px solid #333;
  }

  .cal-toggle {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.5rem;
    background: none;
    border: none;
    color: #aaa;
    cursor: pointer;
    font-size: 0.8rem;
    text-align: left;
  }

  .cal-toggle:hover {
    color: #eee;
  }

  .cal-summary {
    color: #666;
    font-family: monospace;
    font-size: 0.75rem;
    margin-left: auto;
  }

  .cal-panel {
    padding: 0.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .cal-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .cal-row label,
  .cal-row .field-label {
    font-size: 0.8rem;
    color: #aaa;
    min-width: 60px;
  }

  .cal-row .unit {
    color: #666;
    font-size: 0.7rem;
  }

  .cal-row input[type="number"] {
    width: 110px;
    padding: 0.2rem 0.4rem;
    background: #1a1a2e;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    font-family: monospace;
    font-size: 0.8rem;
    text-align: right;
  }

  .flip-toggle {
    padding: 0.15rem 0.6rem;
    border: 1px solid #555;
    border-radius: 4px;
    background: #1a1a2e;
    color: #888;
    cursor: pointer;
    font-size: 0.75rem;
  }

  .flip-toggle.active {
    background: #0f3460;
    border-color: #42a5f5;
    color: #42a5f5;
  }

  .cal-fov {
    font-size: 0.75rem;
    color: #666;
    font-family: monospace;
    text-align: center;
    padding: 0.2rem 0;
  }

  .cal-save {
    padding: 0.3rem 0.75rem;
    border: 1px solid #555;
    border-radius: 4px;
    background: #2a2a4e;
    color: #eee;
    cursor: pointer;
    font-size: 0.8rem;
    align-self: flex-end;
  }

  .cal-save:hover:not(:disabled) {
    background: #3a3a5e;
  }

  .cal-save:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
