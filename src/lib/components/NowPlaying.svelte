<script lang="ts">
  import { nowPlaying, playing, position, monitorFileVolume, errorMsg } from '../stores';
  import { prev, next, play, pause, seek, setMonitorFileVolume } from '../api';

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

  let volTimer: ReturnType<typeof setTimeout> | undefined;
  function onFileVolume(e: Event) {
    const pct = parseInt((e.currentTarget as HTMLInputElement).value, 10);
    monitorFileVolume.set(pct);
    clearTimeout(volTimer);
    volTimer = setTimeout(() => {
      setMonitorFileVolume(pct).catch((e) => errorMsg.set(String(e)));
    }, 120);
  }
</script>

<header class="border-b border-gray-700 px-4 py-3">
  <div class="flex items-center gap-3">
    <div
      class="flex h-11 w-11 shrink-0 items-center justify-center rounded bg-gradient-to-br from-lime-600 to-lime-600 text-lg text-white shadow"
    >
      ♪
    </div>
    <div class="lcd min-w-0 flex-1">
      <div class="truncate text-sm font-medium">
        {$nowPlaying?.title ?? 'Nothing playing'}
      </div>
      <div class="truncate text-xs opacity-80">{$nowPlaying?.artist || '—'}</div>
    </div>
    <button class="rounded p-2 text-gray-300 hover:bg-gray-800" onclick={() => prev()} aria-label="Previous"
      >⏮</button
    >
    <button
      class="flex h-10 w-10 items-center justify-center rounded-full bg-gray-100 text-gray-900 hover:bg-white"
      onclick={toggle}
      aria-label="{$playing ? 'Pause' : 'Play'}"
    >
      {$playing ? '⏸' : '▶'}
    </button>
    <button class="rounded p-2 text-gray-300 hover:bg-gray-800" onclick={() => next()} aria-label="Next"
      >⏭</button
    >
  </div>

  <div class="mt-3 flex items-center gap-2 text-[10px] tabular-nums text-gray-500">
    <span class="w-8 text-right">{fmt(elapsed)}</span>
    <div
      bind:this={trackEl}
      class="group relative h-1.5 flex-1 cursor-pointer rounded-full bg-gray-800"
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
        class="absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-lime-600 to-lime-400"
        style="width: {pct}%"
      ></div>
      <div
        class="absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow ring-2 ring-lime-400 transition-transform group-hover:scale-110"
        style="left: {pct}%"
      ></div>
    </div>
    <span class="w-8">{fmt($position.total_secs)}</span>
  </div>

  <div class="mt-2 flex items-center gap-2">
    <span class="text-[10px] uppercase tracking-wider text-gray-500">File vol</span>
    <input
      type="range"
      min="0"
      max="200"
      step="1"
      value={$monitorFileVolume}
      oninput={onFileVolume}
      class="streamy-range flex-1"
      aria-label="Local file volume"
    />
    <span class="w-10 text-right text-[10px] tabular-nums text-gray-500">{$monitorFileVolume}%</span>
  </div>
</header>

<style>
  .streamy-range {
    accent-color: #10b981;
    height: 1.25rem;
    cursor: pointer;
  }
</style>
