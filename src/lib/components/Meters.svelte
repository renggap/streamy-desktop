<script lang="ts">
  import { levels } from '../stores';

  // Map a 0..1 linear level to a 0..100% bar height (simple, no dB scale yet).
  const bar = (v: number) => `${Math.min(100, Math.max(0, v * 100)).toFixed(1)}%`;
</script>

<div class="flex items-end gap-2">
  <div class="flex flex-col items-center gap-1">
    <div class="relative h-16 w-3 overflow-hidden rounded bg-zinc-800">
      <div
        class="absolute bottom-0 w-full bg-emerald-500"
        style="height: {bar($levels.monitor[0])}"
      ></div>
    </div>
    <span class="text-[10px] text-zinc-500">L</span>
  </div>
  <div class="flex flex-col items-center gap-1">
    <div class="relative h-16 w-3 overflow-hidden rounded bg-zinc-800">
      <div
        class="absolute bottom-0 w-full bg-emerald-500"
        style="height: {bar($levels.monitor[1])}"
      ></div>
    </div>
    <span class="text-[10px] text-zinc-500">R</span>
  </div>
</div>
