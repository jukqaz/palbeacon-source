<script lang="ts">
  import { recentWorkspaceEntries, workspace, workspaceKindLabel } from './workspace';

  const recent = $derived(recentWorkspaceEntries($workspace));
</script>

{#if recent.length > 0}
  <section class="recent" aria-labelledby="recent-title">
    <h2 id="recent-title">최근 본 항목</h2>
    <div>
      {#each recent.slice(0, 6) as entry (`${entry.kind}:${entry.id}`)}
        <a href={entry.href}>
          {#if entry.image_path}<img src={entry.image_path} alt="" width="48" height="48" />{/if}
          <span
            ><small>{workspaceKindLabel(entry.kind)}</small><strong>{entry.name_ko}</strong></span
          >
        </a>
      {/each}
    </div>
  </section>
{/if}

<style>
  .recent {
    display: grid;
    gap: 10px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--ink);
  }

  h2 {
    margin: 0;
    font-size: 0.9rem;
  }

  .recent > div {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 7px;
  }

  a {
    display: grid;
    min-width: 0;
    min-height: 60px;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 9px;
    align-items: center;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface);
    text-decoration: none;
  }

  img {
    width: 48px;
    height: 48px;
    object-fit: contain;
  }

  a span {
    display: grid;
    min-width: 0;
  }

  small {
    color: var(--muted-strong);
    font-size: 0.7rem;
  }

  strong {
    overflow: hidden;
    font-size: 0.8rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 719px) {
    .recent > div {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
