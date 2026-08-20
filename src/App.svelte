<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isDesktop } from '$lib/services/ipc';

  type AppInfo = { version: string; sqliteVersion: string; debug: boolean };

  let info = $state<AppInfo | null>(null);
  let failure = $state<string | null>(null);

  onMount(async () => {
    if (!isDesktop) {
      failure = 'Running outside the desktop host — the backend is not reachable.';
      return;
    }
    try {
      info = await invoke<AppInfo>('app_info');
    } catch (err) {
      failure = err instanceof Error ? err.message : String(err);
    } finally {
      // Reveal the window only once something has been painted.
      await invoke('app_ready').catch(() => {});
    }
  });
</script>

<main>
  <h1>Grimoire</h1>
  <p class="line">The manuscript belongs to the user.</p>

  {#if info}
    <dl>
      <dt>Version</dt>
      <dd>{info.version}</dd>
      <dt>SQLite</dt>
      <dd>{info.sqliteVersion}</dd>
      <dt>Build</dt>
      <dd>{info.debug ? 'debug' : 'release'}</dd>
    </dl>
  {:else if failure}
    <p class="failure">{failure}</p>
  {/if}
</main>

<style>
  main {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 1rem;
    height: 100%;
    padding: 2rem;
    background: #17140f;
    color: #e8dfce;
    text-align: center;
  }

  h1 {
    font-family: Georgia, 'Times New Roman', serif;
    font-size: 2.5rem;
    font-weight: 400;
    letter-spacing: 0.02em;
  }

  .line {
    color: #9c9184;
    font-style: italic;
  }

  dl {
    display: grid;
    grid-template-columns: auto auto;
    gap: 0.25rem 1.5rem;
    margin-top: 1rem;
    font-size: 0.875rem;
    color: #9c9184;
  }

  dt {
    text-align: right;
  }

  dd {
    text-align: left;
    color: #e8dfce;
    font-variant-numeric: tabular-nums;
  }

  .failure {
    color: #d99a76;
    max-width: 32rem;
  }
</style>
