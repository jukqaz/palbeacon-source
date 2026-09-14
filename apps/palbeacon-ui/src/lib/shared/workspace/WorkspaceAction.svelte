<script lang="ts">
  import { scale } from 'svelte/transition';
  import { workspace, toggleWorkspaceEntry, type WorkspaceEntry } from './workspace';

  interface Props {
    entry: WorkspaceEntry;
  }

  let { entry }: Props = $props();
  const saved = $derived(
    $workspace.saved.some(
      (candidate) => candidate.kind === entry.kind && candidate.id === entry.id,
    ),
  );
</script>

<button
  class="workspace-action"
  type="button"
  aria-pressed={saved}
  aria-label={`${entry.name_ko} ${saved ? '저장 해제' : '저장'}`}
  onclick={() => toggleWorkspaceEntry(entry)}
>
  {#key saved}
    <span in:scale={{ duration: 120, start: 0.92 }}>{saved ? '저장됨' : '저장'}</span>
  {/key}
</button>

<style>
  .workspace-action {
    min-width: 56px;
    min-height: 36px;
    padding-inline: 10px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--surface);
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 780;
    transition:
      border-color var(--motion-fast) var(--ease-standard),
      color var(--motion-fast) var(--ease-standard),
      background var(--motion-fast) var(--ease-standard);
  }

  .workspace-action[aria-pressed='true'] {
    border-color: var(--accent);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
  }

  span {
    display: inline-block;
  }
</style>
