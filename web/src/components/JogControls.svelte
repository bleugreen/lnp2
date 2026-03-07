<script>
  import { get } from 'svelte/store';
  import { moveTo, home } from '../lib/api.js';
  import { position, jogFeedrate } from '../lib/stores.js';

  let stepSize = $state(1);
  const steps = [0.1, 1, 10];
  const feedrates = [1000, 3000, 6000, 12000];
  let jogging = $state(false);

  async function jog(axis, direction) {
    if (jogging) return;
    jogging = true;

    try {
      const pos = get(position);
      const delta = stepSize * direction;
      const value = pos[axis] + delta;

      const x = axis === 'x' ? value : undefined;
      const y = axis === 'y' ? value : undefined;
      const z = axis === 'z' ? value : undefined;

      await moveTo(x, y, z, get(jogFeedrate));
    } catch (e) {
      console.error(e);
    } finally {
      jogging = false;
    }
  }

  function handleKeydown(e) {
    if (e.target.tagName === 'INPUT') return;

    switch (e.key) {
      case 'ArrowLeft':  e.preventDefault(); jog('x', -1); break;
      case 'ArrowRight': e.preventDefault(); jog('x', 1); break;
      case 'ArrowUp':    e.preventDefault(); jog('y', 1); break;
      case 'ArrowDown':  e.preventDefault(); jog('y', -1); break;
      case 'PageUp':     e.preventDefault(); jog('z', 1); break;
      case 'PageDown':   e.preventDefault(); jog('z', -1); break;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="jog-controls">
  <h3>Jog</h3>

  <div class="step-selector">
    {#each steps as s}
      <button class:active={stepSize === s} onclick={() => (stepSize = s)}>
        {s}mm
      </button>
    {/each}
  </div>

  <div class="speed-selector">
    {#each feedrates as f}
      <button class:active={$jogFeedrate === f} onclick={() => jogFeedrate.set(f)}>
        {f >= 1000 ? `${f / 1000}k` : f}
      </button>
    {/each}
    <span class="unit">mm/min</span>
  </div>

  <div class="jog-grid">
    <div></div>
    <button class="jog-btn" onclick={() => jog('y', 1)}>Y+</button>
    <div></div>
    <button class="jog-btn" onclick={() => jog('z', 1)}>Z+</button>

    <button class="jog-btn" onclick={() => jog('x', -1)}>X-</button>
    <div class="center-dot"></div>
    <button class="jog-btn" onclick={() => jog('x', 1)}>X+</button>
    <div></div>

    <div></div>
    <button class="jog-btn" onclick={() => jog('y', -1)}>Y-</button>
    <div></div>
    <button class="jog-btn" onclick={() => jog('z', -1)}>Z-</button>
  </div>

  <button class="home-btn" onclick={() => home().catch(console.error)}>
    Home All
  </button>
</div>

<style>
  .jog-controls {
    background: #16213e;
    border-radius: 8px;
    padding: 1rem;
  }

  h3 {
    margin: 0 0 0.75rem;
    font-size: 0.9rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #888;
  }

  .step-selector, .speed-selector {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
    align-items: center;
  }

  .step-selector button, .speed-selector button {
    flex: 1;
    padding: 0.4rem;
    border: 1px solid #444;
    border-radius: 4px;
    background: #1a1a2e;
    color: #eee;
    cursor: pointer;
    font-size: 0.85rem;
  }

  .step-selector button.active, .speed-selector button.active {
    background: #0f3460;
    border-color: #e94560;
  }

  .unit {
    font-size: 0.7rem;
    color: #666;
    white-space: nowrap;
  }

  .jog-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .jog-btn {
    padding: 0.75rem 0.5rem;
    border: 1px solid #444;
    border-radius: 6px;
    background: #1a1a2e;
    color: #eee;
    cursor: pointer;
    font-size: 0.85rem;
    font-weight: 600;
  }

  .jog-btn:hover {
    background: #0f3460;
  }

  .jog-btn:active {
    background: #e94560;
  }

  .center-dot {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .center-dot::after {
    content: '';
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #444;
  }

  .home-btn {
    width: 100%;
    padding: 0.6rem;
    border: 1px solid #c0392b;
    border-radius: 6px;
    background: #1a1a2e;
    color: #e94560;
    cursor: pointer;
    font-size: 0.9rem;
    font-weight: 600;
  }

  .home-btn:hover {
    background: #c0392b;
    color: #fff;
  }
</style>
