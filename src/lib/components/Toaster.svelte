<script lang="ts">
  import { toasts, dismissToast } from '../stores';

  const icons = {
    success:
      'M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z',
    error: 'M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z',
    info: 'M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z',
  };
</script>

<!-- Joko UI toasts: bottom-right stack, white rounded-xl, border-l-4 accent. -->
<div
  class="pointer-events-none fixed bottom-16 right-4 z-[60] flex w-80 flex-col gap-2"
  aria-live="polite"
>
  {#each $toasts as t (t.id)}
    <div
      class="toast-anim pointer-events-auto flex items-start gap-3 rounded-xl border border-gray-600 border-l-4 bg-gray-800 p-4 shadow-lg
        {t.kind === 'success' ? 'border-l-lime-400' : t.kind === 'error' ? 'border-l-red-500 border-red-900/50 bg-red-900/30' : 'border-l-gray-400'}"
      role="status"
    >
      <div class="shrink-0 {t.kind === 'error' ? 'text-red-500' : 'text-lime-400'}">
        <svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d={icons[t.kind]}></path>
        </svg>
      </div>
      <div class="flex-1">
        <p class="text-sm font-medium text-gray-100">{t.title}</p>
        {#if t.detail}<p class="mt-1 text-xs text-gray-400">{t.detail}</p>{/if}
        {#if t.action}
          <button
            class="mt-2 rounded-lg bg-lime-400 px-3 py-1 text-xs font-semibold text-gray-900 transition-colors hover:bg-lime-300"
            onclick={t.action.run}
          >
            {t.action.label}
          </button>
        {/if}
      </div>
      <button
        class="shrink-0 text-gray-400 transition-colors hover:text-gray-300"
        onclick={() => dismissToast(t.id)}
        aria-label="Dismiss"
      >
        <svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
        </svg>
      </button>
    </div>
  {/each}
</div>
