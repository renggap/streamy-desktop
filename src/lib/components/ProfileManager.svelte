<script lang="ts">
  import { profiles, errorMsg } from '../stores';
  import { listProfiles, saveProfile, deleteProfile } from '../api';
  import type { ConnectionProfile } from '../api';

  let editing = $state<ConnectionProfile>(blank());
  let password = $state('');
  let isEditing = $state(false);

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

  function edit(p: ConnectionProfile) {
    editing = { ...p };
    password = '';
    isEditing = true;
  }

  function reset() {
    editing = blank();
    password = '';
    isEditing = false;
  }

  async function save() {
    const exists = $profiles.some((p) => p.name === editing.name);
    if (!editing.name || !editing.host) {
      errorMsg.set('Name and host are required.');
      return;
    }
    if (!exists && !password) {
      errorMsg.set('Password is required for a new profile.');
      return;
    }
    // Blank password on edit -> null -> keep the existing stored password.
    const pw = password.length > 0 ? password : null;
    try {
      await saveProfile({ ...editing }, pw);
      await refresh();
      reset();
    } catch (e) {
      errorMsg.set(String(e));
    }
  }

  async function remove(name: string) {
    try {
      await deleteProfile(name);
      await refresh();
      if (isEditing && editing.name === name) reset();
    } catch (e) {
      errorMsg.set(String(e));
    }
  }
</script>

<div class="space-y-3">
  <h3 class="text-xs font-semibold uppercase tracking-wider text-zinc-500">
    {isEditing ? 'Edit profile' : 'New profile'}
  </h3>
  <div class="grid grid-cols-2 gap-2 text-xs text-zinc-300">
    <label class="flex flex-col gap-1"
      >Name <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.name} disabled={isEditing} /></label
    >
    <label class="flex flex-col gap-1"
      >Stream ID <input type="number" class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.stream_id} /></label
    >
    <label class="flex flex-col gap-1"
      >Host <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.host} /></label
    >
    <label class="flex flex-col gap-1"
      >Port <input type="number" class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.port} /></label
    >
    <label class="col-span-2 flex flex-col gap-1"
      >Password
      <input
        type="password"
        class="rounded bg-zinc-800 px-2 py-1"
        bind:value={password}
        placeholder={isEditing ? '(unchanged)' : ''}
      /></label
    >
    <label class="flex flex-col gap-1"
      >Station name <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.station_name} /></label
    >
    <label class="flex flex-col gap-1"
      >Genre <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.genre} /></label
    >
    <label class="col-span-2 flex flex-col gap-1"
      >URL <input class="rounded bg-zinc-800 px-2 py-1" bind:value={editing.url} /></label
    >
  </div>
  <div class="flex gap-2">
    <button class="rounded bg-emerald-700 px-3 py-1 text-xs text-white hover:bg-emerald-600" onclick={save}
      >{isEditing ? 'Update' : 'Save profile'}</button
    >
    {#if isEditing}
      <button class="rounded bg-zinc-800 px-3 py-1 text-xs text-zinc-300 hover:bg-zinc-700" onclick={reset}
        >Cancel</button
      >
    {/if}
  </div>

  <div class="border-t border-zinc-800 pt-2">
    <div class="mb-1 text-xs font-semibold uppercase tracking-wider text-zinc-500">Saved</div>
    {#each $profiles as p (p.name)}
      <div class="flex items-center justify-between gap-2 py-1 text-xs">
        <span class="truncate text-zinc-300"
          >{p.name} <span class="text-zinc-500">{p.host}:{p.port}</span></span
        >
        <span class="flex shrink-0 gap-2">
          <button class="text-cyan-400 hover:text-cyan-300" onclick={() => edit(p)}>Edit</button>
          <button class="text-red-400 hover:text-red-300" onclick={() => remove(p.name)}>Delete</button>
        </span>
      </div>
    {:else}
      <div class="text-xs text-zinc-600">None yet — fill the form above.</div>
    {/each}
  </div>
</div>
