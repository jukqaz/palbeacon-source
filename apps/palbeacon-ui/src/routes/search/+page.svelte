<script lang="ts">
  import { page } from '$app/state';
  import { browser } from '$app/environment';
  import CatalogPage from '$lib/catalog/CatalogPage.svelte';

  const initialQuery = $derived(browser ? (page.url.searchParams.get('q') ?? '') : '');
</script>

<CatalogPage scope="all" title="통합 검색" {initialQuery} />
