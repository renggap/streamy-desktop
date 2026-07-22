<script lang="ts">
  import { profiles, errorMsg } from '../stores';
  import { listProfiles, saveProfile, deleteProfile } from '../api';
  import type { ConnectionProfile } from '../api';

  let { onClose } = $props<{ onClose: () => void }>();

  let editing = $state<ConnectionProfile>(blank());
  let password = $state('');

  function blank(): ConnectionProfile {
    return {
      name: '',
      host: '',
      port: 8000,
      user: '',
      mount: '/stream',
      protocol: 'ultravox',
      stream_id: 1,
      station_name: 'Streamy',
      genre: '',
      url: '',
      public: true,
    };
  }

  async function refresh() {
    profiles.set(await listProfiles());
  }

  async function save() {
    if (!editing.name || !editing.host || !password) {
      errorMsg.set('Name, host, and password are required.');
      return;
    }
    try {
      await saveProfile({ ...editing }, password);
      await refresh();
      editing = blank();
      password = '';
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  async function remove(name: string) {
    try {
      await deleteProfile(name);
      await refresh();
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div
  class="fixed inset-0 flex items-center justify-center bg-black/60 p-4"
  role="presentation"
>
  <div class="w-full max-w-lg rounded-lg border border-zinc-700 bg-zinc-900 p-4">
    <div class="mb-3 flex items-center justify-between">
      <h3 class="text-sm font-semibold text-zinc-100">Connection profiles</h3>
      <button class="text-zinc-400 hover:text-zinc-200" onclick={onClose}>✕</button>
    </div>

    <div class="grid grid-cols-2 gap-2 text-xs text-zinc-300">
      <label class="flex flex-col gap-1"
        >Name <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.name} /></label
      >
      <label class="flex flex-col gap-1"
        >Stream ID <input
            type="number"
            class="rounded bg-zinc-800 px-2 py-1"
            bind:value={editing.stream_id}
          /></label
      >
      <label class="flex flex-col gap-1"
        >Host <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.host} /></label
      >
      <label class="flex flex-col gap-1"
        >Port <input
            type="number"
            class="rounded bg-zinc-800 px-2 py-1"
            bind:value={editing.port}
          /></label
      >
      <label class="col-span-2 flex flex-col gap-1"
        >Password <input
            type="password"
            class="rounded bg-zinc-800 px-2 py-1"
            bind:value={password}
          /></label
      >
      <label class="flex flex-col gap-1"
        >Station name <input
            class="rounded bg-zinc-800 px-2 py-1"
            bind:value={editing.station_name}
          /></label
      >
      <label class="flex flex-col gap-1"
        >Genre <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.genre} /></label
      >
      <label class="col-span-2 flex flex-col gap-1"
        >URL <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.url} /></label
      >
    </div>

    <div class="mt-3 flex justify-end gap-2">
      <button
        class="rounded bg-emerald-700 px-3 py-1 text-white hover:bg-emerald-600"
        onclick={save}>Save</button
      >
    </div>

    <div class="mt-4 border-t border-zinc-700 pt-2">
      <div class="mb-1 text-zinc-500">Saved</div>
      {#each $profiles as p (p.name)}
        <div class="flex items-center justify-between py-0.5 text-xs">
          <span class="text-zinc-300">{p.name} <span class="text-zinc-500">{p.host}:{p.port}</span></span>
          <button class="text-red-400 hover:text-red-300" onclick={() => remove(p.name)}
            >Delete</button
          >
        </div>
      {:else}
        <div class="text-xs text-zinc-600">None yet.</div>
      {/each}
    </div>
  </div>
</div>
