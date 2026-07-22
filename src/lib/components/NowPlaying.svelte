<script lang="ts">
  import { nowPlaying } from '../stores';

  const fmt = (s: number) => {
    if (!s || s <= 0) return '--:--';
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, '0')}`;
  };
</script>

<div class="flex items-center gap-3 border-b border-zinc-800 px-4 py-3">
  <div class="flex h-10 w-10 items-center justify-center rounded bg-zinc-800 text-zinc-400">
    ♪
  </div>
  <div class="min-w-0 flex-1">
    <div class="truncate text-sm font-medium text-zinc-100">
      {$nowPlaying?.title ?? 'Nothing playing'}
    </div>
    <div class="truncate text-xs text-zinc-400">
      {$nowPlaying?.artist || '—'}
    </div>
  </div>
  {#if $nowPlaying}
    <span class="text-xs text-zinc-500">{fmt($nowPlaying.duration_secs)}</span>
  {/if}
</div>
