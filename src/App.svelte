<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { nowPlaying } from './lib/stores';
  import Playlist from './lib/components/Playlist.svelte';
  import Transport from './lib/components/Transport.svelte';
  import NowPlaying from './lib/components/NowPlaying.svelte';
  import type { Track } from './lib/api';

  onMount(() => {
    const unref = listen<Track>('now_playing', (e) => {
      nowPlaying.set(e.payload);
    });
    return () => {
      unref.then((u) => u());
    };
  });
</script>

<main class="flex h-screen flex-col bg-zinc-950 text-zinc-200">
  <NowPlaying />
  <Playlist />
  <Transport />
</main>
