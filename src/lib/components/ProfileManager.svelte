<script lang="ts">
  import { profiles, errorMsg, notify, view, profileEditing } from '../stores';
  import { listProfiles, deleteProfile } from '../api';
  import type { ConnectionProfile } from '../api';

  async function refresh() {
    profiles.set(await listProfiles());
  }

  function add() {
    profileEditing.set('');
    view.set('profile-editor');
  }

  function edit(p: ConnectionProfile) {
    profileEditing.set(p.name);
    view.set('profile-editor');
  }

  async function remove(name: string) {
    try {
      await deleteProfile(name);
      await refresh();
      notify('info', 'Profile deleted', name);
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-3">
  <div class="flex items-center justify-between">
    <h3 class="joko-label">Profiles</h3>
    <button
      class="rounded-lg bg-lime-600 px-3 py-1.5 text-xs font-semibold text-white shadow-md transition-all duration-200 hover:bg-lime-700 active:scale-95"
      onclick={add}>+ Add profile</button
    >
  </div>
  <div class="space-y-2">
    {#each $profiles as p (p.name)}
      <div class="flex items-center gap-3 rounded-xl border border-gray-600 bg-gray-800 px-3 py-2.5 shadow-sm">
        <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-lime-900/40 text-base">📡</span>
        <div class="min-w-0 flex-1">
          <span class="truncate text-sm font-medium text-gray-100">{p.name}</span>
          <div class="truncate text-[11px] text-gray-400"
            >{p.host}<span class="text-gray-400">:{p.port}</span>{p.mount || '/'}</div
          >
        </div>
        <div class="flex shrink-0 gap-1">
          <button
            class="rounded-lg bg-gray-800 px-2.5 py-1.5 text-[11px] font-medium text-gray-200 transition-colors hover:bg-gray-700"
            onclick={() => edit(p)}>Edit</button
          >
          <button
            class="rounded-lg bg-red-900/30 px-2.5 py-1.5 text-[11px] font-medium text-red-400 transition-colors hover:bg-red-900/50"
            onclick={() => remove(p.name)}>Delete</button
          >
        </div>
      </div>
    {:else}
      <div class="rounded-xl border border-dashed border-gray-500 px-4 py-6 text-center text-xs text-gray-400">
        No saved profiles — tap "Add profile".
      </div>
    {/each}
  </div>
</div>
