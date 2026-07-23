<script lang="ts">
  import WebView2Repair from './WebView2Repair.svelte';

  let open = $state(false);

  // Show once on first launch (webview present ⇒ localStorage available).
  $effect(() => {
    try {
      if (!localStorage.getItem('streamy.wv2_notice_seen')) open = true;
    } catch {
      open = true; // localStorage unavailable — show anyway.
    }
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
        <span class="flex h-11 w-11 items-center justify-center rounded-lg bg-gradient-to-br from-emerald-600 to-cyan-600 text-xl text-white">📡</span>
        <div>
          <div class="bg-gradient-to-r from-emerald-400 to-cyan-400 bg-clip-text text-lg font-extrabold text-transparent">
            Welcome to Streamy Desktop
          </div>
          <div class="text-[10px] uppercase tracking-[0.3em] text-zinc-500">broadcast · mix · record</div>
        </div>
      </div>

      <p class="text-xs leading-relaxed text-zinc-400">
        Streamy runs on Microsoft's <span class="text-zinc-200">WebView2</span> runtime, which is
        built into Windows 10 21H2+ and Windows 11 and is normally auto-installed alongside the app.
      </p>
      <p class="mt-2 text-xs leading-relaxed text-zinc-500">
        If Streamy ever fails to start with a WebView2 message, run the repair below — it downloads
        the official installer with a progress bar and reinstalls the runtime.
      </p>

      <div class="mt-4 border-t border-zinc-800 pt-4">
        <WebView2Repair />
      </div>

      <div class="mt-5 flex justify-end">
        <button
          class="rounded-lg bg-zinc-800 px-4 py-2 text-xs font-semibold text-zinc-200 hover:bg-zinc-700"
          onclick={dismiss}>Got it</button
        >
      </div>
    </div>
  </div>
{/if}
