<script lang="ts">
  import { browser } from '$app/environment';
  import { page } from '$app/state';
  import CatalogPage from '$lib/catalog/CatalogPage.svelte';

  const initialQuery = $derived(browser ? (page.url.searchParams.get('q') ?? '') : '');
  const initialSelectedId = $derived(browser ? (page.url.searchParams.get('id') ?? '') : '');
</script>

<CatalogPage scope="building" title="건축물 도감" {initialQuery} {initialSelectedId} />
