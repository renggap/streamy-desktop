<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { nowPlaying, errorMsg } from './lib/stores';
  import Playlist from './lib/components/Playlist.svelte';
  import Transport from './lib/components/Transport.svelte';
  import NowPlaying from './lib/components/NowPlaying.svelte';
  import type { Track } from './lib/api';

  onMount(() => {
    const np = listen<Track>('now_playing', (e) => {
      nowPlaying.set(e.payload);
    });
    const err = listen<string>('error', (e) => {
      errorMsg.set(e.payload);
    });
    return () => {
      np.then((u) => u());
      err.then((u) => u());
    };
  });
</script>

<main class="flex h-screen flex-col bg-zinc-950 text-zinc-200">
  {#if $errorMsg}
    <div class="flex items-start gap-2 border-b border-red-900 bg-red-950/60 px-4 py-2 text-sm text-red-200">
      <span class="flex-1">{$errorMsg}</span>
      <button class="text-red-300 hover:text-red-100" onclick={() => errorMsg.set(null)}>✕</button>
    </div>
  {/if}
  <NowPlaying />
  <Playlist />
  <Transport />
</main>
