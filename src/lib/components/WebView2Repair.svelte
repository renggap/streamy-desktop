<script lang="ts">
  import { wv2State, wv2Progress, errorMsg } from '../stores';
  import { notify } from '../stores';
  import { repairWebview2 } from '../api';

  const pct = $derived(
    $wv2Progress.total ? ($wv2Progress.downloaded / $wv2Progress.total) * 100 : 0,
  );
  const mb = (b: number) => (b / (1024 * 1024)).toFixed(1);

  async function repair() {
    if ($wv2State === 'downloading' || $wv2State === 'launching') return;
    wv2Progress.set({ downloaded: 0, total: null });
    wv2State.set('downloading');
    notify('info', 'Downloading installer', 'Official Microsoft WebView2 runtime.');
    try {
      await repairWebview2();
      wv2State.set('done');
      notify('success', 'Installer launched', 'Follow the Microsoft prompts.');
    } catch (e) {
      wv2State.set('err');
      errorMsg.set(String(e));
      notify('error', 'Download failed', String(e));
    }
  }
</script>

<div class="space-y-2">
  <button
    class="rounded-lg bg-lime-600 px-3 py-2 text-xs font-semibold text-white shadow-md transition-all duration-200 hover:bg-lime-700 active:scale-95 disabled:opacity-50"
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
      <div class="h-2 w-full overflow-hidden rounded-full bg-gray-100">
        <div
          class="h-full rounded-full bg-gradient-to-r from-lime-400 to-lime-600 transition-[width]"
          style="width: {$wv2Progress.total ? pct : 40}%"
        ></div>
      </div>
      <div class="mt-1 text-[10px] text-gray-500">
        {#if $wv2Progress.total}
          {mb($wv2Progress.downloaded)} / {mb($wv2Progress.total)} MB
        {:else}
          downloading…
        {/if}
      </div>
    </div>
  {/if}

  {#if $wv2State === 'done'}
    <div class="text-[11px] text-lime-600">
      ✓ Installer launched — follow the Microsoft prompts, then relaunch Streamy.
    </div>
  {:else if $wv2State === 'err'}
    <div class="text-[11px] text-red-600">✕ Download failed — check your connection and retry.</div>
  {/if}
</div>
