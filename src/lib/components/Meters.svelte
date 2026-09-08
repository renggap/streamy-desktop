<script lang="ts">
  import { levels } from '../stores';

  // 0..1 linear -> width %.
  const w = (v: number) => `${Math.min(100, Math.max(0, v * 100)).toFixed(1)}%`;
</script>

<div class="flex flex-col gap-1 text-[10px] text-gray-400">
  {#snippet row(label: string, ch: [number, number])}
    <div class="flex items-center gap-1">
      <span class="w-6 shrink-0 text-gray-400">{label}</span>
      <div class="flex flex-1 flex-col gap-0.5">
        <div class="h-1.5 w-full overflow-hidden rounded bg-gray-800">
          <div class="h-full bg-lime-500" style="width: {w(ch[0])}"></div>
        </div>
        <div class="h-1.5 w-full overflow-hidden rounded bg-gray-800">
          <div class="h-full bg-lime-500" style="width: {w(ch[1])}"></div>
        </div>
      </div>
    </div>
  {/snippet}
  {@render row('Mic', $levels.mic)}
  {@render row('Spk', $levels.monitor)}
</div>
