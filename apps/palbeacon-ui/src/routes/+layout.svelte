<script lang="ts">
  import { page } from '$app/state';
  import type { Snippet } from 'svelte';
  import { Toaster } from 'svelte-sonner';
  import '../app.css';
  import { routeForPath } from '$lib/app/navigation/routes';
  import { detectRuntimeContext, type RuntimeContext } from '$lib/app/platform/runtime';
  import { isTauriRuntime } from '$lib/shared/platform/tauri';
  import { PALBEACON_ORIGIN, PALBEACON_SOCIAL_IMAGE, seoForPath } from '$lib/app/seo/seo';
  import AppShell from '$lib/app/shell/AppShell.svelte';
  import NotFoundState from '$lib/app/shell/NotFoundState.svelte';
  import PersonalDataBridge from '$lib/personal/PersonalDataBridge.svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();
  let runtime = $state<RuntimeContext>({ platform: 'web', shell: 'browser', appVersion: null });
  let runtimeReady = $state(false);
  const currentRoute = $derived(routeForPath(page.url.pathname));
  const seo = $derived(
    seoForPath(page.url.pathname, runtimeReady && runtime.platform === 'windows'),
  );
  const canonicalUrl = $derived(new URL(seo.path, PALBEACON_ORIGIN).href);
  const socialImageUrl = new URL(PALBEACON_SOCIAL_IMAGE, PALBEACON_ORIGIN).href;
  const windowsOnly = $derived(
    currentRoute?.availability.length === 1 && currentRoute.availability[0] === 'windows',
  );
  const platformUnavailable = $derived(
    runtimeReady &&
      currentRoute !== undefined &&
      !currentRoute.availability.includes(runtime.platform),
  );

  $effect(() => {
    let active = true;
    void (async () => {
      try {
        const context = await detectRuntimeContext();
        if (active) {
          runtime = context;
          runtimeReady = true;
        }
      } catch {
        if (active) runtimeReady = true;
      }
    })();
    return () => {
      active = false;
    };
  });
</script>

<svelte:head>
  <title>{seo.title}</title>
  <meta name="description" content={seo.description} />
  <meta name="robots" content={seo.index ? 'index, follow' : 'noindex, nofollow'} />
  <link rel="canonical" href={canonicalUrl} />
  <meta property="og:type" content="website" />
  <meta property="og:site_name" content="PalBeacon" />
  <meta property="og:locale" content="ko_KR" />
  <meta property="og:title" content={seo.title} />
  <meta property="og:description" content={seo.description} />
  <meta property="og:url" content={canonicalUrl} />
  <meta property="og:image" content={socialImageUrl} />
  <meta property="og:image:width" content="1024" />
  <meta property="og:image:height" content="1024" />
  <meta property="og:image:alt" content="PalBeacon 앱 마크" />
  <meta name="twitter:card" content="summary" />
  <meta name="twitter:title" content={seo.title} />
  <meta name="twitter:description" content={seo.description} />
  <meta name="twitter:image" content={socialImageUrl} />
</svelte:head>

<span data-palbeacon-hydrated={runtimeReady ? 'true' : 'false'} hidden></span>
<a class="skip-link" href="#main-content">본문으로 건너뛰기</a>
<Toaster theme="dark" position="bottom-right" visibleToasts={2} closeButton />
<PersonalDataBridge enabled={runtimeReady && runtime.platform === 'windows'} />
<AppShell {runtime} pwaEnabled={!isTauriRuntime()}>
  {#if windowsOnly && !runtimeReady}
    <section class="platform-state" aria-live="polite">
      <span>ROUTE CHECK</span>
      <h1>경로를 확인하고 있습니다.</h1>
    </section>
  {:else if platformUnavailable}
    <NotFoundState path={page.url.pathname} showPath={false} />
  {:else}
    {@render children()}
  {/if}
</AppShell>

<style>
  .skip-link {
    position: fixed;
    z-index: 100;
    top: 8px;
    left: 8px;
    translate: 0 -180%;
    padding: 9px 12px;
    border: 2px solid var(--focus);
    border-radius: 4px;
    color: var(--void);
    background: var(--focus);
    font-weight: 800;
  }

  .skip-link:focus {
    translate: 0;
  }

  .platform-state {
    display: grid;
    min-height: calc(100vh - 65px);
    place-content: center;
    justify-items: center;
    padding: 32px;
    text-align: center;
  }

  .platform-state span {
    color: var(--brass);
    font-size: 0.75rem;
    font-weight: 850;
    letter-spacing: 0.16em;
  }

  .platform-state h1 {
    max-width: 720px;
    margin: 12px 0 8px;
    font-size: clamp(1.7rem, 4vw, 3rem);
  }
</style>
