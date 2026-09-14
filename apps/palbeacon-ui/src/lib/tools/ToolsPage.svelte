<script lang="ts">
  import { onMount } from 'svelte';
  import AsyncRouteState from '$lib/shared/ui/AsyncRouteState.svelte';
  import { loadToolsCatalog } from './tools';
  import ToolsBoard from './ToolsBoard.svelte';
  import type { ToolMode, ToolsCatalog } from './types';

  interface Props {
    initialMode?: ToolMode;
    initialBuildingId?: string;
    initialTargetId?: string;
    initialOwnedOnly?: boolean;
  }
  let {
    initialMode = 'team',
    initialBuildingId = '',
    initialTargetId = '',
    initialOwnedOnly = false,
  }: Props = $props();
  let catalog = $state<ToolsCatalog | null>(null);
  let failed = $state(false);

  const load = async () => {
    catalog = null;
    failed = false;
    try {
      catalog = await loadToolsCatalog();
    } catch (cause) {
      console.error('Tools data load failed', cause);
      failed = true;
    }
  };
  onMount(() => void load());
</script>

{#if catalog}
  <ToolsBoard {catalog} {initialMode} {initialBuildingId} {initialTargetId} {initialOwnedOnly} />
{:else if failed}
  <AsyncRouteState
    state="error"
    title="도구를 준비하지 못했습니다."
    message="잠시 후 다시 시도해 주세요."
    accent="brass"
    onRetry={load}
  />
{:else}
  <AsyncRouteState
    state="loading"
    title="계산 도구를 준비하고 있습니다."
    message="잠시만 기다려 주세요."
    accent="brass"
  />
{/if}
