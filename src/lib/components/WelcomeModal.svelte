<script lang="ts">
  import { onMount } from 'svelte';
  import WebView2Repair from './WebView2Repair.svelte';
  import { webview2Installed } from '../api';

  let open = $state(false);

  // Only show in the genuine "runtime not installed" case. If WebView2 is present
  // (the normal case — the app couldn't run otherwise), suppress + remember.
  onMount(async () => {
    try {
      if (localStorage.getItem('streamy.wv2_notice_seen')) return;
    } catch {
      /* localStorage unavailable */
    }
    try {
      const ok = await webview2Installed();
      if (ok) {
        try {
          localStorage.setItem('streamy.wv2_notice_seen', '1');
        } catch {
          /* ignore */
        }
        return;
      }
    } catch {
      // Detection command failed — don't bother the user; Repair stays in Settings.
      return;
    }
    open = true;
  });

  function dismiss() {
    try {
      localStorage.setItem('streamy.wv2_notice_seen', '1');
    } catch {
      /* ignore */
    }
    open = false;
  }
</script>

{#if open}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
    <div class="retro-chrome w-full max-w-md rounded-xl p-6">
      <div class="mb-3 flex items-center gap-3">
        <span class="flex h-11 w-11 items-center justify-center rounded-lg bg-gradient-to-br from-lime-600 to-lime-600 text-xl text-white">📡</span>
        <div>
          <div class="bg-gradient-to-r from-lime-300 to-lime-400 bg-clip-text text-lg font-extrabold text-transparent">
            Welcome to Streamy Desktop
          </div>
          <div class="text-[10px] uppercase tracking-[0.3em] text-gray-500">broadcast · mix · record</div>
        </div>
      </div>

      <p class="text-xs leading-relaxed text-amber-300">
        The Microsoft WebView2 runtime wasn't found on this PC. Streamy needs it to run.
      </p>
      <p class="mt-2 text-xs leading-relaxed text-gray-500">
        Click repair below — it downloads the official installer with a progress bar and installs
        the runtime. Then relaunch Streamy.
      </p>

      <div class="mt-4 border-t border-gray-700 pt-4">
        <WebView2Repair />
      </div>

      <div class="mt-5 flex justify-end">
        <button
          class="rounded-lg bg-gray-800 px-4 py-2 text-xs font-semibold text-gray-200 hover:bg-gray-700"
          onclick={dismiss}>Dismiss</button
        >
      </div>
    </div>
  </div>
{/if}
