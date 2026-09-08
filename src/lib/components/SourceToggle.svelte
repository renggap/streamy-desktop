<script lang="ts">
  import { sourceMode } from '../stores';
  import { notify } from '../stores';
  import { setSourceMode } from '../api';
  import type { SourceMode } from '../api';

  const modes: SourceMode[] = ['file', 'mic', 'both'];

  function pick(m: SourceMode) {
    sourceMode.set(m);
    setSourceMode(m);
    notify('info', 'Source', `Switched to ${m}`);
  }
</script>

<div class="flex items-center gap-1 text-xs">
  {#each modes as m (m)}
    <button
      class="rounded-lg px-2.5 py-1.5 font-medium capitalize transition-colors {$sourceMode === m
        ? 'bg-lime-600 text-white shadow-md'
        : 'bg-gray-100 text-gray-700 hover:bg-gray-200'}"
      onclick={() => pick(m)}>{m}</button
    >
  {/each}
</div>
