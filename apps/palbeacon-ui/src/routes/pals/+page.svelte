<script lang="ts">
  import { page } from '$app/state';
  import { browser } from '$app/environment';
  import CatalogPage from '$lib/catalog/CatalogPage.svelte';

  const initialQuery = $derived(browser ? (page.url.searchParams.get('q') ?? '') : '');
  const initialSelectedId = $derived(browser ? (page.url.searchParams.get('id') ?? '') : '');
  const initialOwnedOnly = $derived(browser && page.url.searchParams.get('mine') === '1');
</script>

<CatalogPage scope="pal" title="팰 도감" {initialQuery} {initialSelectedId} {initialOwnedOnly} />
