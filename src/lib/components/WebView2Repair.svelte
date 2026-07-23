<script lang="ts">
  import { wv2State, wv2Progress, errorMsg } from '../stores';
  import { repairWebview2 } from '../api';

  const pct = $derived(
    $wv2Progress.total ? ($wv2Progress.downloaded / $wv2Progress.total) * 100 : 0,
  );
  const mb = (b: number) => (b / (1024 * 1024)).toFixed(1);

  async function repair() {
    if ($wv2State === 'downloading' || $wv2State === 'launching') return;
    wv2Progress.set({ downloaded: 0, total: null });
    wv2State.set('downloading');
    try {
      await repairWebview2();
      wv2State.set('done');
    } catch (e) {
      wv2State.set('err');
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-2">
  <button
    class="rounded-lg bg-gradient-to-b from-cyan-600 to-cyan-800 px-3 py-2 text-xs font-semibold text-white hover:from-cyan-500 hover:to-cyan-700 active:scale-95 disabled:opacity-50"
    onclick={repair}
    disabled={$wv2State === 'downloading' || $wv2State === 'launching'}
  >
    {#if $wv2State === 'downloading' || $wv2State === 'launching'}
      Preparing…
    {:else}
      Repair WebView2 runtime
    {/if}
  </button>

  {#if $wv2State === 'downloading'}
    <div>
      <div class="h-2 w-full overflow-hidden rounded-full bg-zinc-800">
        <div
          class="h-full rounded-full bg-gradient-to-r from-cyan-500 to-emerald-500 transition-[width]"
          style="width: {$wv2Progress.total ? pct : 40}%"
        ></div>
      </div>
      <div class="mt-1 text-[10px] text-zinc-500">
        {#if $wv2Progress.total}
          {mb($wv2Progress.downloaded)} / {mb($wv2Progress.total)} MB
        {:else}
          downloading…
        {/if}
      </div>
    </div>
  {/if}

  {#if $wv2State === 'done'}
    <div class="text-[11px] text-emerald-400">
      ✓ Installer launched — follow the Microsoft prompts, then relaunch Streamy.
    </div>
  {:else if $wv2State === 'err'}
    <div class="text-[11px] text-red-400">✕ Download failed — check your connection and retry.</div>
  {/if}
</div>
