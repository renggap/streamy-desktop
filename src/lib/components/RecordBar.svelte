<script lang="ts">
  import { recording, recFormat, recordingPath, recStartedAt, errorMsg } from '../stores';
  import { notify } from '../stores';
  import { startRecording, stopRecording } from '../api';

  let elapsed = $state(0);

  // Tick the elapsed indicator once per second while recording.
  $effect(() => {
    if (!$recording) {
      elapsed = 0;
      return;
    }
    const start = $recStartedAt ?? Date.now();
    elapsed = Math.floor((Date.now() - start) / 1000);
    const id = setInterval(() => {
      elapsed = Math.floor((Date.now() - start) / 1000);
    }, 1000);
    return () => clearInterval(id);
  });

  async function toggle() {
    if ($recording) {
      try {
        await stopRecording();
        notify('info', 'Recording stopped', $recordingPath ?? undefined);
      } catch (e) {
        errorMsg.set(String(e));
      }
      return;
    }
    try {
      await startRecording($recFormat);
      notify('success', 'Recording started', $recFormat.toUpperCase());
    } catch (e) {
      errorMsg.set(String(e));
      notify('error', 'Record failed', String(e));
    }
  }

  function fmt(s: number): string {
    const m = Math.floor(s / 60);
    const r = s % 60;
    return `${m}:${r.toString().padStart(2, '0')}`;
  }

  const label = $derived($recording ? '■ Stop' : '● Rec');
</script>

<div class="space-y-2">
  <div class="flex items-center gap-2 text-xs">
    <select
      class="rounded-lg border border-gray-600 bg-gray-900 px-2 py-1.5 text-gray-100 focus:border-lime-500"
      bind:value={$recFormat}
      disabled={$recording}
    >
      <option value="mp3">MP3</option>
      <option value="wav">WAV</option>
    </select>
    <button
      class="rounded-lg px-3 py-1.5 font-semibold shadow-md transition-all duration-200 active:scale-95 {$recording
        ? 'bg-gray-700 text-gray-200 hover:bg-gray-600'
        : 'bg-red-600 text-white hover:bg-red-700'}"
      onclick={toggle}
    >
      {label}
    </button>
    {#if $recording}
      <span class="ml-auto font-mono text-red-400">{fmt(elapsed)}</span>
    {/if}
  </div>
  {#if $recording && $recordingPath}
    <div class="truncate text-[11px] text-gray-400">→ {$recordingPath}</div>
  {/if}
</div>
