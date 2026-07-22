<script lang="ts">
  import { next, prev, pause, play, seek } from '../api';
  import { playing } from '../stores';

  let pos = $state(0); // 0..100

  function onSeek() {
    seek(pos / 100);
  }

  function togglePlay() {
    if ($playing) {
      pause();
      playing.set(false);
    } else {
      play();
      playing.set(true);
    }
  }
</script>

<div class="flex flex-col gap-2 border-t border-zinc-800 px-4 py-3">
  <div class="flex items-center justify-center gap-2">
    <button
      class="rounded bg-zinc-800 px-3 py-1.5 text-sm text-zinc-200 hover:bg-zinc-700"
      onclick={() => prev()}
    >⏮ Prev</button>
    <button
      class="min-w-16 rounded bg-zinc-700 px-4 py-1.5 text-sm font-medium text-zinc-100 hover:bg-zinc-600"
      onclick={togglePlay}
    >{$playing ? '⏸ Pause' : '▶ Play'}</button>
    <button
      class="rounded bg-zinc-800 px-3 py-1.5 text-sm text-zinc-200 hover:bg-zinc-700"
      onclick={() => next()}
    >⏭ Next</button>
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
