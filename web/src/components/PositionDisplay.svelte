<script>
  import { position } from '../lib/stores.js';
</script>

<div class="position-display">
  <h3>Position</h3>
  <div class="axes">
    <div class="axis"><span class="label">X</span><span class="value">{$position.x.toFixed(3)}</span></div>
    <div class="axis"><span class="label">Y</span><span class="value">{$position.y.toFixed(3)}</span></div>
    <div class="axis"><span class="label">Z</span><span class="value">{$position.z.toFixed(3)}</span></div>
    <div class="axis"><span class="label">A</span><span class="value">{$position.a.toFixed(3)}</span></div>
    <div class="axis"><span class="label">B</span><span class="value">{$position.b.toFixed(3)}</span></div>
  </div>
</div>

<style>
  .position-display {
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

  .axes {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .axis {
    display: flex;
    justify-content: space-between;
    font-family: 'SF Mono', 'Fira Code', monospace;
  }

  .label {
    color: #e94560;
    font-weight: 600;
    width: 1.5rem;
  }

  .value {
    color: #eee;
    text-align: right;
  }
</style>
