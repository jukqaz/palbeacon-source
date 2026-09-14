<script lang="ts">
  import { navigating, page } from '$app/state';
  import {
    BookOpen01Icon,
    Home01Icon,
    MapsLocation01Icon,
    Task01Icon,
  } from '@hugeicons/core-free-icons';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import type { Snippet } from 'svelte';
  import {
    commonPrimaryDestinations,
    isNavigationTargetActive,
    resolvePalPath,
    routeForPath,
    secondaryNavigationFor,
    utilityDestinations,
    windowsPrimaryDestinations,
  } from '$lib/app/navigation/routes';
  import type { RuntimeContext } from '$lib/app/platform/runtime';
  import PwaExperience from '$lib/app/pwa/PwaExperience.svelte';
  import WorkspaceTray from '$lib/shared/workspace/WorkspaceTray.svelte';
  import PixelMark from './PixelMark.svelte';

  interface Props {
    runtime: RuntimeContext;
    pwaEnabled?: boolean;
    children: Snippet;
  }

  let { runtime, pwaEnabled = true, children }: Props = $props();
  const currentRoute = $derived(routeForPath(page.url.pathname));
  const currentGroup = $derived(currentRoute?.group ?? 'explore');
  const commonPrimaryRoutes = $derived(commonPrimaryDestinations(runtime.platform));
  const windowsPrimaryRoutes = $derived(windowsPrimaryDestinations(runtime.platform));
  const utilityRoutes = $derived(utilityDestinations(runtime.platform));
  const visibleUtilityRoutes = $derived(
    utilityRoutes.filter(
      (destination) =>
        destination.id !== 'search' ||
        (currentRoute?.id !== 'explore.home' && currentRoute?.id !== 'explore.search'),
    ),
  );
  const secondaryNavigation = $derived(secondaryNavigationFor(currentGroup, runtime.platform));

  const compactPrimaryIcon = (id: string) => {
    if (id === 'home') return Home01Icon;
    if (id === 'map') return MapsLocation01Icon;
    if (id === 'catalog') return BookOpen01Icon;
    if (id === 'plan') return Task01Icon;
    return null;
  };
</script>

<div class="shell">
  {#if navigating.to !== null}<div
      class="navigation-progress"
      role="progressbar"
      aria-label="페이지 이동 중"
    ></div>{/if}

  <header class="topbar">
    <a class="brand" href={resolvePalPath('/')} aria-label="PalBeacon 홈">
      <PixelMark size={28} />
      <strong>PALBEACON</strong>
    </a>

    <div class="navigation-groups">
      <nav class="primary" aria-label="주요 메뉴">
        {#each commonPrimaryRoutes as destination (destination.id)}
          {@const compactIcon = compactPrimaryIcon(destination.id)}
          <a
            href={resolvePalPath(destination.path)}
            class:compact-primary={destination.visibility === 'compact'}
            class:active={isNavigationTargetActive(destination, currentRoute)}
            aria-current={isNavigationTargetActive(destination, currentRoute) ? 'page' : undefined}
          >
            {#if compactIcon}
              <span class="primary-icon" aria-hidden="true">
                <HugeiconsIcon icon={compactIcon} size={23} strokeWidth={1.8} />
              </span>
            {/if}
            <span class="primary-label">{destination.label_ko}</span>
          </a>
        {/each}
      </nav>

      {#if windowsPrimaryRoutes.length > 0}
        <nav class="platform-primary" aria-label="PC 메뉴">
          {#each windowsPrimaryRoutes as destination (destination.id)}
            <a
              href={resolvePalPath(destination.path)}
              class:active={isNavigationTargetActive(destination, currentRoute)}
              aria-current={isNavigationTargetActive(destination, currentRoute)
                ? 'page'
                : undefined}
            >
              <span>{destination.label_ko}</span>
            </a>
          {/each}
        </nav>
      {/if}
    </div>

    <div class="utilities">
      <a href={resolvePalPath('/fan-content-notice/')} aria-label="비공식 팬 프로젝트 이용 고지">
        비공식
      </a>
      {#if visibleUtilityRoutes.length > 0}
        <nav aria-label="보조 메뉴">
          {#each visibleUtilityRoutes as destination (destination.id)}
            <a
              href={resolvePalPath(destination.path)}
              class:active={isNavigationTargetActive(destination, currentRoute)}
              aria-current={isNavigationTargetActive(destination, currentRoute)
                ? 'page'
                : undefined}>{destination.label_ko}</a
            >
          {/each}
        </nav>
      {/if}
    </div>
  </header>

  <PwaExperience enabled={pwaEnabled} />

  {#if secondaryNavigation}
    <div class="section-subnav">
      <nav aria-label={`${secondaryNavigation.label} 세부 메뉴`}>
        {#each secondaryNavigation.items as destination (destination.id)}
          <a
            href={resolvePalPath(destination.path)}
            class:active={isNavigationTargetActive(destination, currentRoute)}
            aria-current={isNavigationTargetActive(destination, currentRoute) ? 'page' : undefined}
            >{destination.label_ko}</a
          >
        {/each}
      </nav>
    </div>
  {/if}

  <WorkspaceTray />
  <main id="main-content" class="content" class:with-secondary={secondaryNavigation !== null}>
    {#key page.url.pathname}
      <div class="route-stage" data-testid="route-stage">
        {@render children()}
      </div>
    {/key}
  </main>
</div>

<style>
  .shell {
    min-height: 100vh;
    min-height: 100dvh;
  }

  .navigation-progress {
    position: fixed;
    z-index: 120;
    top: 0;
    left: 0;
    width: min(42vw, 480px);
    height: 3px;
    background: var(--accent);
    transform-origin: left;
    animation: navigation-progress 700ms var(--ease-emphasized) infinite alternate;
  }

  .topbar {
    position: sticky;
    z-index: 50;
    top: 0;
    display: grid;
    min-height: 64px;
    grid-template-columns: 200px minmax(264px, 1fr) auto;
    align-items: stretch;
    border-bottom: 1px solid var(--border);
    background: var(--void);
  }

  .brand {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 9px;
    padding-inline: 16px;
    border-right: 1px solid var(--border);
    text-decoration: none;
  }

  .brand strong {
    min-width: 0;
    overflow: hidden;
    font-size: 0.88rem;
    letter-spacing: 0.08em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .primary {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: stretch;
    padding-inline: 8px;
  }

  .navigation-groups {
    display: flex;
    min-width: 0;
    align-items: stretch;
  }

  .primary a,
  .platform-primary a {
    position: relative;
    display: inline-flex;
    min-width: 84px;
    align-items: center;
    justify-content: center;
    padding-inline: 12px;
    color: var(--muted-strong);
    text-decoration: none;
    font-size: 0.83rem;
    font-weight: 760;
    white-space: nowrap;
  }

  .primary a:hover,
  .primary a.active,
  .platform-primary a:hover,
  .platform-primary a.active {
    color: var(--text);
    background: var(--surface);
    box-shadow: inset 0 -3px 0 var(--accent);
  }

  .primary .compact-primary {
    display: none;
  }

  .primary-icon {
    display: none;
  }

  .platform-primary {
    display: flex;
    align-items: stretch;
    border-left: 1px solid var(--border-strong);
  }

  .platform-primary a {
    text-decoration: none;
  }

  .utilities {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-inline: 10px;
  }

  .utilities a {
    display: inline-flex;
    min-height: 40px;
    align-items: center;
    padding-inline: 10px;
    border: 1px solid transparent;
    border-radius: 5px;
    color: var(--muted-strong);
    text-decoration: none;
    font-size: 0.75rem;
    font-weight: 760;
    white-space: nowrap;
  }

  .utilities a:hover,
  .utilities a.active {
    border-color: var(--border-strong);
    color: var(--text);
    background: var(--surface);
  }

  .section-subnav {
    position: sticky;
    z-index: 42;
    top: 65px;
    overflow-x: auto;
    padding-inline: clamp(12px, 3vw, 32px);
    border-bottom: 1px solid var(--border);
    background: var(--ink);
    scrollbar-width: none;
  }

  .section-subnav::-webkit-scrollbar {
    display: none;
  }

  .section-subnav nav {
    display: flex;
    width: max-content;
    min-width: 0;
    align-items: stretch;
  }

  .section-subnav a {
    display: inline-flex;
    min-width: 108px;
    min-height: 46px;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    padding-inline: 15px;
    border-right: 1px solid var(--border);
    color: var(--muted-strong);
    text-decoration: none;
    font-size: 0.78rem;
    font-weight: 760;
    white-space: nowrap;
  }

  .section-subnav a:last-child {
    border-right: 0;
  }

  .section-subnav a:hover,
  .section-subnav a.active {
    color: var(--text);
    background: var(--surface);
    box-shadow: inset 0 -3px 0 var(--accent);
  }

  .content {
    min-width: 0;
    min-height: calc(100vh - 65px);
    min-height: calc(100dvh - 65px);
  }

  .content.with-secondary {
    min-height: calc(100vh - 112px);
    min-height: calc(100dvh - 112px);
  }

  .route-stage {
    min-width: 0;
    animation: route-content-enter var(--motion-slow) var(--ease-emphasized) both;
  }

  @keyframes navigation-progress {
    from {
      opacity: 0.55;
      scale: 0.18 1;
    }
    to {
      opacity: 1;
      scale: 1 1;
    }
  }

  @keyframes route-content-enter {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (max-width: 1100px) {
    .topbar {
      grid-template-columns: auto minmax(252px, 1fr) auto;
    }

    .brand {
      border-right: 0;
    }

    .primary {
      justify-content: center;
      padding-inline: 0;
    }

    .primary a {
      min-width: 72px;
    }
  }

  @media (max-width: 719px) {
    .topbar {
      min-height: 58px;
      grid-template-columns: minmax(0, 1fr) auto;
    }

    .navigation-groups {
      display: contents;
    }

    .platform-primary {
      display: none;
    }

    .brand {
      padding-inline: 12px;
    }

    .brand strong {
      font-size: 0.8rem;
    }

    .primary {
      position: fixed;
      z-index: 70;
      right: 0;
      bottom: 0;
      left: 0;
      min-height: calc(68px + env(safe-area-inset-bottom));
      align-items: flex-start;
      justify-content: stretch;
      padding: 4px max(4px, env(safe-area-inset-right)) env(safe-area-inset-bottom)
        max(4px, env(safe-area-inset-left));
      border-top: 1px solid var(--border-strong);
      background: var(--sidebar);
    }

    .primary .compact-primary {
      display: inline-flex;
    }

    .primary a {
      position: relative;
      flex-direction: column;
      flex: 1;
      min-width: 0;
      min-height: 64px;
      gap: 2px;
      padding: 6px 4px 4px;
      font-size: 0.75rem;
      line-height: 1;
    }

    .primary a:hover,
    .primary a.active {
      background: transparent;
      box-shadow: none;
    }

    .primary a.active {
      color: var(--text);
      background: color-mix(in srgb, var(--accent), transparent 92%);
    }

    .primary a.active::before {
      position: absolute;
      top: -5px;
      left: 50%;
      width: 34px;
      height: 3px;
      background: var(--accent);
      content: '';
      transform: translateX(-50%);
    }

    .primary-icon {
      display: grid;
      width: 28px;
      height: 28px;
      place-items: center;
      color: var(--muted-strong);
    }

    .primary a.active .primary-icon {
      color: var(--accent);
    }

    .primary-label {
      font-size: 0.75rem;
      font-weight: 760;
      line-height: 1;
    }

    .utilities {
      padding-right: 8px;
    }

    .section-subnav {
      top: 58px;
    }

    .section-subnav a {
      min-width: 88px;
      min-height: 44px;
      padding-inline: 12px;
    }

    .content {
      min-height: calc(100vh - 120px);
      min-height: calc(100dvh - 120px);
      padding-bottom: calc(68px + env(safe-area-inset-bottom));
    }

    .content.with-secondary {
      min-height: calc(100vh - 164px);
      min-height: calc(100dvh - 164px);
    }
  }

  @media (max-width: 359px) {
    .brand strong {
      display: none;
    }

    .brand {
      width: 48px;
      justify-content: center;
      padding-inline: 8px;
    }
  }

  @media (max-width: 719px) and (display-mode: standalone) {
    .topbar {
      min-height: calc(58px + env(safe-area-inset-top));
      padding-top: env(safe-area-inset-top);
    }

    .section-subnav {
      top: calc(58px + env(safe-area-inset-top));
    }

    .content {
      padding-bottom: calc(68px + env(safe-area-inset-bottom));
    }
  }
</style>
