<script>
  import { updateCameraConfig } from '../lib/api.js';

  let { camera = '', config = {}, onUpdate = () => {} } = $props();

  let expanded = $state(false);
  let saving = $state(false);
  let flipX = $state(false);
  let flipY = $state(false);

  // Sync local state when props change
  $effect(() => {
    if (config) {
      flipX = config.flip_x || false;
      flipY = config.flip_y || false;
    }
  });

  async function save() {
    saving = true;
    try {
      await updateCameraConfig(camera, {
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
      {#if config}
        <div class="cal-info">
          <span>UPP: {config.upp_x?.toFixed(4)} × {config.upp_y?.toFixed(4)} mm/px</span>
          <span>FOV: {(config.upp_x * config.width).toFixed(1)} × {(config.upp_y * config.height).toFixed(1)} mm</span>
        </div>
      {/if}
      <div class="cal-row">
        <span class="field-label">Flip X</span>
        <button
          class="flip-toggle"
          class:active={flipX}
          onclick={() => { flipX = !flipX; save(); }}
        >{flipX ? 'ON' : 'OFF'}</button>
      </div>
      <div class="cal-row">
        <span class="field-label">Flip Y</span>
        <button
          class="flip-toggle"
          class:active={flipY}
          onclick={() => { flipY = !flipY; save(); }}
        >{flipY ? 'ON' : 'OFF'}</button>
      </div>
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

  .cal-info {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    font-size: 0.75rem;
    color: #666;
    font-family: monospace;
    padding-bottom: 0.3rem;
    border-bottom: 1px solid #333;
    margin-bottom: 0.2rem;
  }

  .cal-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .cal-row .field-label {
    font-size: 0.8rem;
    color: #aaa;
    min-width: 60px;
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
</style>
