<script lang="ts">
  import { onMount } from 'svelte';
  import AsyncRouteState from '$lib/shared/ui/AsyncRouteState.svelte';
  import { loadUnifiedCatalog } from './catalog';
  import BuildingCatalogBoard from './BuildingCatalogBoard.svelte';
  import CatalogBoard from './CatalogBoard.svelte';
  import ItemCatalogBoard from './ItemCatalogBoard.svelte';
  import PalCatalogBoard from './PalCatalogBoard.svelte';
  import SkillTypeNavigation from './SkillTypeNavigation.svelte';
  import type { CatalogKind, UnifiedCatalog } from './types';

  interface Props {
    scope: CatalogKind | 'all';
    title: string;
    initialQuery?: string;
    initialSelectedId?: string;
    initialOwnedOnly?: boolean;
  }

  let {
    scope,
    title,
    initialQuery = '',
    initialSelectedId = '',
    initialOwnedOnly = false,
  }: Props = $props();
  let catalog = $state<UnifiedCatalog | null>(null);
  let failed = $state(false);

  const load = async () => {
    catalog = null;
    failed = false;
    try {
      catalog = await loadUnifiedCatalog();
    } catch (cause) {
      console.error('Catalog load failed', cause);
      failed = true;
    }
  };

  onMount(() => void load());
</script>

{#if scope === 'active_skill' || scope === 'passive_skill'}
  <SkillTypeNavigation active={scope === 'active_skill' ? 'active' : 'passive'} />
{/if}

{#if catalog}
  {#if scope === 'pal'}
    <PalCatalogBoard {catalog} {title} {initialQuery} {initialSelectedId} {initialOwnedOnly} />
  {:else if scope === 'item'}
    <ItemCatalogBoard {catalog} {title} {initialQuery} {initialSelectedId} />
  {:else if scope === 'building'}
    <BuildingCatalogBoard {catalog} {title} {initialQuery} {initialSelectedId} />
  {:else}
    <CatalogBoard {catalog} {scope} {title} {initialQuery} {initialSelectedId} />
  {/if}
{:else if failed}
  <AsyncRouteState
    state="error"
    title="카탈로그를 불러오지 못했습니다."
    message="잠시 후 다시 시도해 주세요."
    onRetry={load}
  />
{:else}
  <AsyncRouteState
    state="loading"
    title="도감을 불러오고 있습니다."
    message="잠시만 기다려 주세요."
  />
{/if}
