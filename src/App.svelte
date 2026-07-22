<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { listen } from '@tauri-apps/api/event';
  import { nowPlaying, currentIndex, playing, errorMsg, playlist } from './lib/stores';
  import { next } from './lib/api';
  import Playlist from './lib/components/Playlist.svelte';
  import Transport from './lib/components/Transport.svelte';
  import NowPlaying from './lib/components/NowPlaying.svelte';
  import type { Track } from './lib/api';

  onMount(() => {
    const np = listen<Track>('now_playing', (e) => {
      nowPlaying.set(e.payload);
      playing.set(true);
      // Sync the row highlight to whatever is now playing (covers next/prev/auto-advance).
      const idx = get(playlist).findIndex((t) => t.path === e.payload.path);
      currentIndex.set(idx >= 0 ? idx : null);
    });
    const ended = listen('track_ended', () => {
      playing.set(false);
      next(); // auto-advance; if at end, next() is a no-op
    });
    const err = listen<string>('error', (e) => {
      errorMsg.set(e.payload);
    });
    return () => {
      np.then((u) => u());
      ended.then((u) => u());
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
