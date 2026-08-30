<script lang="ts">
  import { sourceMode } from '../stores';
  import { setSourceMode } from '../api';
  import type { SourceMode } from '../api';

  const modes: SourceMode[] = ['file', 'mic', 'both'];

  function pick(m: SourceMode) {
    sourceMode.set(m);
    setSourceMode(m);
  }
</script>

<div class="flex items-center gap-1 text-xs">
  <span class="mr-1 text-gray-500">Source</span>
  {#each modes as m (m)}
    <button
      class="rounded px-2 py-1 capitalize {$sourceMode === m
        ? 'bg-gray-100 text-gray-900'
        : 'bg-gray-800 text-gray-300 hover:bg-gray-700'}"
      onclick={() => pick(m)}>{m}</button
    >
  {/each}
</div>
