<script lang="ts">
  import { playlist, currentIndex } from '../stores';
  import { addToPlaylist, playIndex, openAudioFiles } from '../api';

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

<div class="flex items-center justify-between px-4 py-2">
  <h2 class="text-xs font-semibold uppercase tracking-wider text-zinc-500">Playlist</h2>
  <button
    class="rounded bg-zinc-800 px-3 py-1 text-xs text-zinc-200 hover:bg-zinc-700"
    onclick={addFiles}
  >
    + Add files
  </button>
</div>

<div class="min-h-0 flex-1 overflow-y-auto">
  {#each $playlist as track, i (track.path)}
    <button
      class="flex w-full items-center gap-3 px-4 py-2 text-left hover:bg-zinc-800/60
             {$currentIndex === i ? 'bg-zinc-800' : ''}"
      onclick={() => select(i)}
    >
      <span class="w-5 shrink-0 text-right text-xs text-zinc-500">{i + 1}</span>
      <span class="min-w-0 flex-1 truncate text-sm text-zinc-200">{track.title}</span>
      <span class="shrink-0 text-xs text-zinc-500">{fmt(track.duration_secs)}</span>
    </button>
  {:else}
    <div class="px-4 py-8 text-center text-sm text-zinc-600">
      No tracks. Click “Add files”.
    </div>
  {/each}
</div>
