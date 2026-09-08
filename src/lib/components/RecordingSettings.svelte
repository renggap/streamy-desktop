<script lang="ts">
  import { recDir, errorMsg } from '../stores';
  import { notify } from '../stores';
  import { setRecordingDir, openFolder } from '../api';

  async function browse() {
    const dir = await openFolder();
    if (!dir) return;
    try {
      await setRecordingDir(dir);
      recDir.set(dir);
      notify('success', 'Recording folder set', dir);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  async function reset() {
    try {
      await setRecordingDir(null);
      recDir.set(null);
      notify('info', 'Recording folder reset', 'Back to app data default.');
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-2 text-xs">
  <div class="flex items-center justify-between gap-2">
    <div class="min-w-0">
      <div class="truncate text-gray-700">{$recDir ?? 'Default (app data)'}</div>
    </div>
    <div class="flex shrink-0 gap-2">
      <button
        class="rounded-lg bg-gray-100 px-3 py-1.5 font-semibold text-gray-800 shadow-sm transition-colors hover:bg-gray-200"
        onclick={browse}
        >Browse…</button
      >
      {#if $recDir}
        <button
          class="rounded-lg bg-gray-100 px-3 py-1.5 font-semibold text-gray-500 shadow-sm transition-colors hover:bg-gray-200"
          onclick={reset}
          >Reset</button
        >
      {/if}
    </div>
  </div>
</div>
