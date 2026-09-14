<script lang="ts">
  import { onMount } from 'svelte';
  import { toast } from 'svelte-sonner';
  import {
    loadNativeOverlayControlDocument,
    updateNativeOverlayControl,
  } from '$lib/connection/native';
  import type { NativeOverlayControlDocument } from '$lib/connection/types';
  import { isTauriRuntime } from '$lib/shared/platform/tauri';
  import AsyncRouteState from '$lib/shared/ui/AsyncRouteState.svelte';
  import { loadMapBundle } from './data';
  import MapBoard from './MapBoard.svelte';
  import { publicSupplementalFilterIds } from './map';
  import { enabledMapFilterIds, withMapFilterIds } from './overlay-filters';
  import type { MapBundle } from './types';

  interface Props {
    initialQuery?: string;
    loadBundle?: () => Promise<MapBundle>;
    loadOverlay?: () => Promise<NativeOverlayControlDocument>;
    updateOverlay?: (
      expectedVersion: number,
      settings: NativeOverlayControlDocument['settings'],
    ) => Promise<NativeOverlayControlDocument>;
    nativeRuntime?: boolean;
  }

  let {
    initialQuery = '',
    loadBundle = loadMapBundle,
    loadOverlay = loadNativeOverlayControlDocument,
    updateOverlay = updateNativeOverlayControl,
    nativeRuntime = isTauriRuntime(),
  }: Props = $props();
  let bundle = $state<MapBundle | null>(null);
  let failed = $state(false);
  let loading = $state(true);
  let overlayDocument = $state<NativeOverlayControlDocument | null>(null);
  let overlayUpdateQueue: Promise<void> = Promise.resolve();

  const load = async () => {
    loading = true;
    failed = false;
    try {
      const [nextBundle, nextOverlayDocument] = await Promise.all([
        loadBundle(),
        nativeRuntime ? loadOverlay().catch(() => null) : Promise.resolve(null),
      ]);
      overlayDocument = nextOverlayDocument;
      bundle = nextBundle;
    } catch (reason) {
      console.error('Map data load failed', reason);
      bundle = null;
      failed = true;
    } finally {
      loading = false;
    }
  };

  onMount(() => {
    void load();
  });

  const syncOverlayFilters = (enabledFilterIds: readonly string[]) => {
    if (overlayDocument === null) return;
    const requestedIds = new Set(enabledFilterIds);
    const availableSupplementalIds = new Set(bundle ? publicSupplementalFilterIds(bundle) : []);
    const previousUpdate = overlayUpdateQueue;
    overlayUpdateQueue = (async () => {
      try {
        await previousUpdate;
        if (overlayDocument === null) return;
        overlayDocument = await updateOverlay(
          overlayDocument.version,
          withMapFilterIds(overlayDocument.settings, requestedIds, availableSupplementalIds),
        );
      } catch {
        toast.error('오버레이 지도 표시를 적용하지 못했습니다.');
        overlayDocument = await loadOverlay().catch(() => null);
      }
    })();
  };
</script>

{#if bundle}
  <MapBoard
    {bundle}
    {initialQuery}
    initialEnabledFilterIds={overlayDocument
      ? enabledMapFilterIds(overlayDocument.settings)
      : undefined}
    onEnabledFilterIdsChange={syncOverlayFilters}
  />
{:else if loading}
  <AsyncRouteState
    state="loading"
    title="지도를 준비하고 있습니다."
    message="잠시만 기다려 주세요."
  />
{:else if failed}
  <AsyncRouteState
    state="error"
    title="지도를 열지 못했습니다."
    message="잠시 후 다시 시도해 주세요."
    onRetry={load}
  />
{/if}
