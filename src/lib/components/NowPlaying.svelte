<script lang="ts">
  import { nowPlaying, playing, position } from '../stores';
  import { prev, next, play, pause, seek } from '../api';

  let scrubbing = $state(false);
  let scrubPct = $state(0);
  let trackEl = $state<HTMLDivElement>();

  const fmt = (s: number) => {
    if (!s || !isFinite(s)) return '0:00';
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, '0')}`;
  };

  const pct = $derived(scrubbing ? scrubPct : $position.pct * 100);
  const elapsed = $derived(scrubbing ? (scrubPct / 100) * $position.total_secs : $position.elapsed_secs);

  function pctFromEvent(e: PointerEvent) {
    const rect = trackEl?.getBoundingClientRect();
    if (!rect) return 0;
    return Math.min(100, Math.max(0, ((e.clientX - rect.left) / rect.width) * 100));
  }
  function down(e: PointerEvent) {
    scrubbing = true;
    scrubPct = pctFromEvent(e);
    trackEl?.setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (scrubbing) scrubPct = pctFromEvent(e);
  }
  function up() {
    if (!scrubbing) return;
    scrubbing = false;
    seek(scrubPct / 100);
  }
  function toggle() {
    if ($playing) {
      pause();
      playing.set(false);
    } else {
      play();
      playing.set(true);
    }
  }
</script>

<header class="border-b border-zinc-800 px-4 py-3">
  <div class="flex items-center gap-3">
    <div
      class="flex h-11 w-11 shrink-0 items-center justify-center rounded bg-gradient-to-br from-emerald-600 to-cyan-600 text-lg text-white shadow"
    >
      ♪
    </div>
    <div class="min-w-0 flex-1">
      <div class="truncate text-sm font-medium text-zinc-100">
        {$nowPlaying?.title ?? 'Nothing playing'}
      </div>
      <div class="truncate text-xs text-zinc-400">{$nowPlaying?.artist || '—'}</div>
    </div>
    <button class="rounded p-2 text-zinc-300 hover:bg-zinc-800" onclick={() => prev()} aria-label="Previous"
      >⏮</button
    >
    <button
      class="flex h-10 w-10 items-center justify-center rounded-full bg-zinc-100 text-zinc-900 hover:bg-white"
      onclick={toggle}
      aria-label="{$playing ? 'Pause' : 'Play'}"
    >
      {$playing ? '⏸' : '▶'}
    </button>
    <button class="rounded p-2 text-zinc-300 hover:bg-zinc-800" onclick={() => next()} aria-label="Next"
      >⏭</button
    >
  </div>

  <div class="mt-3 flex items-center gap-2 text-[10px] tabular-nums text-zinc-500">
    <span class="w-8 text-right">{fmt(elapsed)}</span>
    <div
      bind:this={trackEl}
      class="group relative h-1.5 flex-1 cursor-pointer rounded-full bg-zinc-800"
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
      onpointercancel={up}
      role="slider"
      tabindex="0"
      aria-label="Seek"
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={pct}
    >
      <div
        class="absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-emerald-500 to-cyan-400"
        style="width: {pct}%"
      ></div>
      <div
        class="absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow ring-2 ring-cyan-400 transition-transform group-hover:scale-110"
        style="left: {pct}%"
      ></div>
    </div>
    <span class="w-8">{fmt($position.total_secs)}</span>
  </div>
</header>
