<script lang="ts">
  import { playlist, currentIndex, playing } from '../stores';
  import { playIndex, addToPlaylist, openAudioFiles } from '../api';

  async function addFiles() {
    const paths = await openAudioFiles();
    if (paths.length === 0) return;
    const tracks = await addToPlaylist(paths);
    playlist.set(tracks);
  }

  function select(i: number) {
    currentIndex.set(i);
    playIndex(i);
  }

  const fmt = (s: number) => {
    if (!s || s <= 0) return '--:--';
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, '0')}`;
  };
</script>

<div class="flex flex-col gap-3 p-4">
  <!-- Header -->
  <div class="retro-chrome flex items-center justify-between rounded-lg px-4 py-3">
    <div>
      <div class="text-xs font-semibold uppercase tracking-[0.2em] text-gray-400">Playlist</div>
      <div class="text-[10px] text-gray-500">{$playlist.length} {$playlist.length === 1 ? 'track' : 'tracks'}</div>
    </div>
    <button
      class="rounded-lg bg-gradient-to-b from-lime-600 to-lime-700 px-3 py-1.5 text-xs font-semibold text-white hover:from-lime-300 hover:to-lime-600 active:scale-95"
      onclick={addFiles}
    >
      + Add files
    </button>
  </div>

  <!-- Tracks -->
  <div class="retro-chrome overflow-hidden rounded-lg">
    {#each $playlist as track, i (track.path)}
      <button
        class="group flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-gray-800/60 {$currentIndex === i
          ? 'bg-gray-800/80'
          : ''}"
        onclick={() => select(i)}
      >
        <!-- index / playing indicator -->
        <span class="w-6 shrink-0 text-center">
          {#if $currentIndex === i && $playing}
            <span class="now-playing" aria-label="playing"><i></i><i></i><i></i></span>
          {:else}
            <span class="text-xs tabular-nums text-gray-500 group-hover:text-gray-400">{i + 1}</span>
          {/if}
        </span>
        <span class="min-w-0 flex-1">
          <span class="block truncate text-sm {$currentIndex === i ? 'text-lime-300' : 'text-gray-200'}">{track.title}</span>
          {#if track.artist}<span class="block truncate text-[11px] text-gray-500">{track.artist}</span>{/if}
        </span>
        <span class="shrink-0 text-xs tabular-nums text-gray-500">{fmt(track.duration_secs)}</span>
      </button>
    {:else}
      <div class="flex flex-col items-center gap-3 px-4 py-12 text-center">
        <span class="text-3xl opacity-40">🎵</span>
        <div class="text-sm text-gray-500">No tracks yet</div>
        <div class="text-[11px] text-gray-500">Drag audio files onto the window, or</div>
        <button
          class="rounded-lg bg-gray-800 px-3 py-1.5 text-xs text-gray-200 hover:bg-gray-700"
          onclick={addFiles}>Add files</button
        >
      </div>
    {/each}
  </div>
</div>

<style>
  .now-playing {
    display: inline-flex;
    align-items: flex-end;
    gap: 2px;
    height: 14px;
  }
  .now-playing i {
    width: 3px;
    background: #22d3ee;
    border-radius: 1px;
    animation: np-bounce 0.9s ease-in-out infinite;
  }
  .now-playing i:nth-child(1) { height: 50%; animation-delay: 0s; }
  .now-playing i:nth-child(2) { height: 100%; animation-delay: 0.2s; }
  .now-playing i:nth-child(3) { height: 70%; animation-delay: 0.4s; }
  @keyframes np-bounce {
    0%, 100% { transform: scaleY(0.4); }
    50% { transform: scaleY(1); }
  }
  @media (prefers-reduced-motion: reduce) {
    .now-playing i { animation: none; }
  }
</style>
