<script lang="ts">
  import { next, prev, pause, seek } from '../api';

  let pos = $state(0); // 0..100

  function onSeek() {
    seek(pos / 100);
  }
</script>

<div class="flex flex-col gap-2 border-t border-zinc-800 px-4 py-3">
  <div class="flex items-center gap-2">
    <button
      class="rounded bg-zinc-800 px-3 py-1.5 text-sm text-zinc-200 hover:bg-zinc-700"
      onclick={() => prev()}
    >⏮ Prev</button>
    <button
      class="rounded bg-zinc-800 px-3 py-1.5 text-sm text-zinc-200 hover:bg-zinc-700"
      onclick={() => pause()}
    >⏸ Pause</button>
    <button
      class="rounded bg-zinc-800 px-3 py-1.5 text-sm text-zinc-200 hover:bg-zinc-700"
      onclick={() => next()}
    >⏭ Next</button>
    <span class="ml-auto text-xs text-zinc-500">Click a track to play</span>
  </div>
  <input
    type="range"
    min="0"
    max="100"
    step="0.1"
    bind:value={pos}
    onchange={onSeek}
    class="w-full accent-zinc-400"
    aria-label="Seek"
  />
</div>
