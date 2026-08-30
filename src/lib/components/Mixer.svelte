<script lang="ts">
  import { crossfade, eqLow, eqMid, eqHigh, duckEnabled, duckThreshold, duckDepth } from '../stores';
  import { setCrossfade, setEq, setDuck } from '../api';

  function onEq() {
    setEq($eqLow, $eqMid, $eqHigh);
  }
  function onCrossfade() {
    setCrossfade($crossfade);
  }
  function onDuck() {
    setDuck($duckEnabled, $duckThreshold, $duckDepth);
  }
</script>

<div class="space-y-3">
  <!-- 3-band EQ: vertical-range sliders, -12..+12 dB -->
  <div>
    <div class="mb-1 text-xs font-semibold uppercase tracking-wider text-gray-500">EQ</div>
    <div class="flex items-end justify-center gap-6">
      <label class="flex flex-col items-center gap-1 text-[10px] text-gray-400">
        <input type="range" min="-12" max="12" step="1" class="eq-range" bind:value={$eqLow} onchange={onEq} />
        Low
      </label>
      <label class="flex flex-col items-center gap-1 text-[10px] text-gray-400">
        <input type="range" min="-12" max="12" step="1" class="eq-range" bind:value={$eqMid} onchange={onEq} />
        Mid
      </label>
      <label class="flex flex-col items-center gap-1 text-[10px] text-gray-400">
        <input type="range" min="-12" max="12" step="1" class="eq-range" bind:value={$eqHigh} onchange={onEq} />
        High
      </label>
    </div>
  </div>

  <!-- Crossfader A <-> B -->
  <div>
    <div class="mb-1 flex items-center justify-between text-[10px] text-gray-500">
      <span>DECK A</span><span class="font-semibold uppercase tracking-wider text-gray-500">Crossfade</span><span>DECK B</span>
    </div>
    <input type="range" min="0" max="1" step="0.01" class="xfade-range" bind:value={$crossfade} onchange={onCrossfade} />
  </div>

  <!-- Mic auto-duck -->
  <div class="border-t border-gray-700 pt-2">
    <label class="flex items-center justify-between text-xs text-gray-300">
      <span>Mic auto-duck</span>
      <input type="checkbox" bind:checked={$duckEnabled} onchange={onDuck} />
    </label>
    {#if $duckEnabled}
      <div class="mt-2 grid grid-cols-2 gap-2 text-[10px] text-gray-400">
        <label class="flex flex-col gap-1">
          Threshold {$duckThreshold.toFixed(2)}
          <input type="range" min="0.02" max="0.5" step="0.01" bind:value={$duckThreshold} onchange={onDuck} />
        </label>
        <label class="flex flex-col gap-1">
          Depth {$duckDepth} dB
          <input type="range" min="3" max="24" step="1" bind:value={$duckDepth} onchange={onDuck} />
        </label>
      </div>
    {/if}
  </div>
</div>

<style>
  .eq-range {
    writing-mode: vertical-lr;
    direction: rtl;
    width: 1.5rem;
    height: 5rem;
    accent-color: #10b981;
  }
  .xfade-range {
    width: 100%;
    accent-color: #06b6d4;
  }
</style>
