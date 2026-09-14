<script lang="ts">
  import { flip } from 'svelte/animate';
  import { fade, fly } from 'svelte/transition';
  import { removeWorkspaceEntry, workspace, workspaceKindLabel } from './workspace';
</script>

{#if $workspace.saved.length > 0}
  <aside class="workspace-tray" aria-label="내 목록">
    <header>
      <h2>내 목록</h2>
    </header>
    <div class="saved-list">
      {#each $workspace.saved as entry (`${entry.kind}:${entry.id}`)}
        <article
          animate:flip={{ duration: 160 }}
          in:fly={{ y: 6, duration: 180 }}
          out:fade={{ duration: 120 }}
        >
          <a href={entry.href}>
            {#if entry.image_path}<img src={entry.image_path} alt="" width="36" height="36" />{/if}
            <span
              ><small>{workspaceKindLabel(entry.kind)}</small><strong>{entry.name_ko}</strong></span
            >
          </a>
          <button
            type="button"
            aria-label={`${entry.name_ko} 내 목록에서 제거`}
            onclick={() => removeWorkspaceEntry(entry)}>×</button
          >
        </article>
      {/each}
    </div>
  </aside>
{/if}

<style>
  .workspace-tray {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 12px;
    align-items: center;
    margin: 0 clamp(12px, 2vw, 28px) 16px;
    padding: 10px;
    border: 1px solid var(--border-strong);
    border-radius: 7px;
    background: var(--ink);
  }

  header {
    display: flex;
    align-items: baseline;
    gap: 7px;
    padding-inline: 5px;
  }

  h2 {
    margin: 0;
    font-size: 0.82rem;
  }

  small {
    color: var(--muted-strong);
    font-size: 0.7rem;
  }

  .saved-list {
    display: flex;
    min-width: 0;
    gap: 7px;
    overflow-x: auto;
  }

  article {
    display: grid;
    min-width: 166px;
    grid-template-columns: minmax(0, 1fr) 32px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface);
  }

  a {
    display: grid;
    min-width: 0;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 7px;
    align-items: center;
    padding: 6px;
    text-decoration: none;
  }

  img {
    width: 36px;
    height: 36px;
    object-fit: contain;
  }

  a span {
    display: grid;
    min-width: 0;
  }

  strong {
    overflow: hidden;
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  article > button {
    min-width: 32px;
    border: 0;
    border-left: 1px solid var(--border);
    color: var(--muted-strong);
    background: transparent;
    cursor: pointer;
  }

  @media (max-width: 719px) {
    .workspace-tray {
      grid-template-columns: 1fr;
      margin-bottom: calc(78px + env(safe-area-inset-bottom));
    }
  }
</style>
