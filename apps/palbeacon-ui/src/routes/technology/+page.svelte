<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { browser } from '$app/environment';
  import AsyncRouteState from '$lib/shared/ui/AsyncRouteState.svelte';
  import { loadTechnologyCatalog } from '$lib/technology/catalog';
  import TechnologyBoard from '$lib/technology/TechnologyBoard.svelte';
  import type { TechnologyCatalog } from '$lib/technology/types';

  let catalog = $state<TechnologyCatalog | null>(null);
  let failed = $state(false);
  let loadController: AbortController | null = null;
  const initialQuery = $derived(browser ? (page.url.searchParams.get('q') ?? '') : '');
  const initialSelectedId = $derived(browser ? (page.url.searchParams.get('id') ?? '') : '');

  const load = async () => {
    loadController?.abort();
    const controller = new AbortController();
    loadController = controller;
    catalog = null;
    failed = false;
    try {
      catalog = await loadTechnologyCatalog((input, init) =>
        fetch(input, { ...init, signal: controller.signal }),
      );
    } catch {
      if (controller.signal.aborted) return;
      failed = true;
    }
  };

  onMount(() => {
    void load();
    return () => {
      loadController?.abort();
      loadController = null;
    };
  });
</script>

{#if catalog}
  <TechnologyBoard {catalog} {initialQuery} {initialSelectedId} />
{:else if failed}
  <AsyncRouteState
    state="error"
    title="기술 데이터를 불러오지 못했습니다."
    message="잠시 후 다시 시도해 주세요."
    onRetry={load}
  />
{:else}
  <AsyncRouteState
    state="loading"
    title="기술 도감을 불러오고 있습니다."
    message="잠시만 기다려 주세요."
  />
{/if}
