<script lang="ts">
  import { recDir, errorMsg } from '../stores';
  import { setRecordingDir, openFolder } from '../api';

  async function browse() {
    const dir = await openFolder();
    if (!dir) return;
    try {
      await setRecordingDir(dir);
      recDir.set(dir);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  async function reset() {
    try {
      await setRecordingDir(null);
      recDir.set(null);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-2 text-xs">
  <div class="flex items-center justify-between gap-2">
    <div class="min-w-0">
      <div class="truncate text-gray-300">{$recDir ?? 'Default (app data)'}</div>
    </div>
    <div class="flex shrink-0 gap-2">
      <button class="rounded bg-gray-800 px-3 py-1.5 text-gray-200 hover:bg-gray-700" onclick={browse}
        >Browse…</button
      >
      {#if $recDir}
        <button class="rounded bg-gray-800 px-3 py-1.5 text-gray-400 hover:bg-gray-700" onclick={reset}
          >Reset</button
        >
      {/if}
    </div>
  </div>
</div>
