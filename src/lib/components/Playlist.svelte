<script lang="ts">
  import { playlist, currentIndex, playing } from '../stores';
  import { notify } from '../stores';
  import { playIndex, addToPlaylist, openAudioFiles } from '../api';

  async function addFiles() {
    const paths = await openAudioFiles();
    if (paths.length === 0) return;
    const tracks = await addToPlaylist(paths);
    playlist.set(tracks);
    notify('success', 'Added to playlist', `${paths.length} file${paths.length === 1 ? '' : 's'}`);
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
  <div class="joko-card flex items-center justify-between rounded-xl px-4 py-3">
    <div>
      <div class="joko-label">Playlist</div>
      <div class="text-[10px] text-gray-400">{$playlist.length} {$playlist.length === 1 ? 'track' : 'tracks'}</div>
    </div>
    <button
      class="rounded-lg bg-lime-600 px-3 py-1.5 text-xs font-semibold text-white shadow-md transition-all duration-200 hover:bg-lime-700 active:scale-95"
      onclick={addFiles}
    >
      + Add files
    </button>
  </div>

  <!-- Tracks -->
  <div class="joko-card overflow-hidden rounded-xl">
    {#each $playlist as track, i (track.path)}
      <button
        class="group flex w-full items-center gap-3 border-b border-gray-700/60 px-4 py-2.5 text-left transition-colors last:border-0 hover:bg-lime-900/30/60 {$currentIndex === i
          ? 'bg-lime-900/30'
          : ''}"
        onclick={() => select(i)}
      >
        <!-- index / playing indicator -->
        <span class="w-6 shrink-0 text-center">
          {#if $currentIndex === i && $playing}
            <span class="now-playing" aria-label="playing"><i></i><i></i><i></i></span>
          {:else}
            <span class="text-xs tabular-nums text-gray-400 group-hover:text-gray-300">{i + 1}</span>
          {/if}
        </span>
        <span class="min-w-0 flex-1">
          <span class="block truncate text-sm {$currentIndex === i ? 'font-medium text-lime-300' : 'text-gray-100'}">{track.title}</span>
          {#if track.artist}<span class="block truncate text-[11px] text-gray-400">{track.artist}</span>{/if}
        </span>
        <span class="shrink-0 text-xs tabular-nums text-gray-400">{fmt(track.duration_secs)}</span>
      </button>
    {:else}
      <div class="flex flex-col items-center gap-3 px-4 py-12 text-center">
        <span class="text-3xl opacity-40">🎵</span>
        <div class="text-sm text-gray-400">No tracks yet</div>
        <div class="text-[11px] text-gray-400">Drag audio files onto the window, or</div>
        <button
          class="rounded-lg bg-gray-800 px-3 py-1.5 text-xs font-semibold text-gray-200 shadow-sm transition-colors hover:bg-gray-700"
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
    background: #65a30d; /* lime-600 */
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
