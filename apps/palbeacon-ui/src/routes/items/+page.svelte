<script lang="ts">
  import { page } from '$app/state';
  import { browser } from '$app/environment';
  import CatalogPage from '$lib/catalog/CatalogPage.svelte';

  const initialQuery = $derived(browser ? (page.url.searchParams.get('q') ?? '') : '');
  const initialSelectedId = $derived(browser ? (page.url.searchParams.get('id') ?? '') : '');
</script>

<CatalogPage scope="item" title="아이템 도감" {initialQuery} {initialSelectedId} />
