<script lang="ts">
  import { base } from '$app/paths';
  import { onMount } from 'svelte';
  import {
    DownloadSquare01Icon,
    Refresh01Icon,
    WifiDisconnected02Icon,
  } from '@hugeicons/core-free-icons';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import StatusBadge from '$lib/app/shell/StatusBadge.svelte';

  interface Props {
    enabled?: boolean;
  }

  interface InstallPromptEvent extends Event {
    prompt: () => Promise<void>;
    userChoice: Promise<{ outcome: 'accepted' | 'dismissed'; platform: string }>;
  }

  type Experience = 'hidden' | 'offline' | 'update' | 'install' | 'ios';

  let { enabled = true }: Props = $props();
  let online = $state(true);
  let standalone = $state(false);
  let ios = $state(false);
  let installDismissed = $state(false);
  let iosHelpOpen = $state(false);
  let installPrompt = $state<InstallPromptEvent | null>(null);
  let updateWorker = $state<ServiceWorker | null>(null);
  let updating = $state(false);
  let retrying = $state(false);
  let refreshRequested = false;

  const experience = $derived<Experience>(
    !enabled
      ? 'hidden'
      : !online
        ? 'offline'
        : updateWorker
          ? 'update'
          : standalone || installDismissed
            ? 'hidden'
            : installPrompt
              ? 'install'
              : ios
                ? 'ios'
                : 'hidden',
  );
  const icon = $derived(
    experience === 'offline'
      ? WifiDisconnected02Icon
      : experience === 'update'
        ? Refresh01Icon
        : DownloadSquare01Icon,
  );

  const dismissInstall = () => {
    installDismissed = true;
    iosHelpOpen = false;
    sessionStorage.setItem('palbeacon:pwa-install-dismissed', 'true');
  };

  const install = async () => {
    if (!installPrompt) return;
    const prompt = installPrompt;
    installPrompt = null;
    await prompt.prompt();
    const choice = await prompt.userChoice;
    if (choice.outcome === 'dismissed') dismissInstall();
  };

  const applyUpdate = () => {
    if (!updateWorker) return;
    updating = true;
    refreshRequested = true;
    updateWorker.postMessage({ type: 'SKIP_WAITING' }, []);
  };

  const retryConnection = async () => {
    retrying = true;
    try {
      await fetch(`${base}/robots.txt?connectivity=${Date.now()}`, {
        cache: 'no-store',
      });
      online = true;
      window.location.reload();
    } catch {
      online = false;
    } finally {
      retrying = false;
    }
  };

  onMount(() => {
    if (!enabled) return;

    let active = true;
    const cleanups: Array<() => void> = [];
    const displayMode = window.matchMedia('(display-mode: standalone)');

    standalone =
      displayMode.matches ||
      Boolean((navigator as Navigator & { standalone?: boolean }).standalone);
    ios =
      /iphone|ipad|ipod/i.test(navigator.userAgent) ||
      (/mac/i.test(navigator.userAgent) && navigator.maxTouchPoints > 1);
    online = navigator.onLine;
    installDismissed = sessionStorage.getItem('palbeacon:pwa-install-dismissed') === 'true';

    const updateOnlineState = () => {
      online = navigator.onLine;
    };
    const updateDisplayMode = () => {
      standalone =
        displayMode.matches ||
        Boolean((navigator as Navigator & { standalone?: boolean }).standalone);
    };
    const captureInstallPrompt = (event: Event) => {
      event.preventDefault();
      installPrompt = event as InstallPromptEvent;
    };
    const installed = () => {
      standalone = true;
      installPrompt = null;
    };
    const controllerChanged = () => {
      if (!refreshRequested) return;
      refreshRequested = false;
      window.location.reload();
    };
    const workerMessage = (event: MessageEvent<unknown>) => {
      const message = event.data as { type?: string; online?: boolean } | null;
      if (message?.type === 'NETWORK_STATUS' && typeof message.online === 'boolean') {
        online = message.online;
      }
    };

    window.addEventListener('online', updateOnlineState);
    window.addEventListener('offline', updateOnlineState);
    window.addEventListener('beforeinstallprompt', captureInstallPrompt);
    window.addEventListener('appinstalled', installed);
    displayMode.addEventListener('change', updateDisplayMode);
    cleanups.push(
      () => window.removeEventListener('online', updateOnlineState),
      () => window.removeEventListener('offline', updateOnlineState),
      () => window.removeEventListener('beforeinstallprompt', captureInstallPrompt),
      () => window.removeEventListener('appinstalled', installed),
      () => displayMode.removeEventListener('change', updateDisplayMode),
    );

    if ('serviceWorker' in navigator) {
      navigator.serviceWorker.addEventListener('controllerchange', controllerChanged);
      navigator.serviceWorker.addEventListener('message', workerMessage);
      cleanups.push(
        () => navigator.serviceWorker.removeEventListener('controllerchange', controllerChanged),
        () => navigator.serviceWorker.removeEventListener('message', workerMessage),
      );
      navigator.serviceWorker.controller?.postMessage({ type: 'GET_NETWORK_STATUS' }, []);

      if (import.meta.env.DEV) {
        void navigator.serviceWorker
          .getRegistrations()
          .then((registrations) =>
            Promise.all(registrations.map((registration) => registration.unregister())),
          );
      } else {
        void navigator.serviceWorker
          .register(`${base}/service-worker.js`)
          .then((registration) => {
            if (!active) return undefined;

            const revealWaitingWorker = () => {
              if (registration.waiting && navigator.serviceWorker.controller) {
                updateWorker = registration.waiting;
              }
            };
            const watchInstallingWorker = () => {
              const worker = registration.installing;
              if (!worker) return;
              const stateChanged = () => {
                if (worker.state === 'installed' && navigator.serviceWorker.controller) {
                  updateWorker = registration.waiting ?? worker;
                }
              };
              worker.addEventListener('statechange', stateChanged);
              cleanups.push(() => worker.removeEventListener('statechange', stateChanged));
            };

            revealWaitingWorker();
            registration.addEventListener('updatefound', watchInstallingWorker);
            cleanups.push(() =>
              registration.removeEventListener('updatefound', watchInstallingWorker),
            );
            return registration.update();
          })
          .catch(() => undefined);
      }
    }

    return () => {
      active = false;
      for (const cleanup of cleanups) cleanup();
    };
  });
</script>

{#if experience !== 'hidden'}
  <section
    class="pwa-notice {experience}"
    role="status"
    aria-live="polite"
    aria-atomic="true"
    data-testid="pwa-experience"
  >
    <span class="pwa-icon" aria-hidden="true">
      <HugeiconsIcon {icon} size={22} strokeWidth={1.8} />
    </span>

    <div class="pwa-copy">
      {#if experience === 'offline'}
        <div class="status-line"><StatusBadge tone="warning">오프라인</StatusBadge></div>
        <strong>연결 없이 저장된 데이터를 보고 있습니다.</strong>
        <p>새 검색 결과와 온라인 기능은 연결이 돌아오면 다시 사용할 수 있습니다.</p>
      {:else if experience === 'update'}
        <div class="status-line"><StatusBadge tone="warning">업데이트</StatusBadge></div>
        <strong>새 PalBeacon 버전을 준비했습니다.</strong>
        <p>지금 적용하면 현재 화면을 한 번 새로고침합니다.</p>
      {:else if experience === 'install'}
        <div class="status-line"><StatusBadge>앱 설치</StatusBadge></div>
        <strong>홈 화면에서 앱처럼 바로 여세요.</strong>
        <p>주소창 없이 열고, 저장된 도감은 연결이 없어도 볼 수 있습니다.</p>
      {:else}
        <div class="status-line"><StatusBadge>홈 화면 추가</StatusBadge></div>
        <strong>PalBeacon을 홈 화면에 추가할 수 있습니다.</strong>
        {#if iosHelpOpen}
          <ol>
            <li>Safari 하단의 공유 버튼을 누릅니다.</li>
            <li>‘홈 화면에 추가’를 선택합니다.</li>
            <li>오른쪽 위 ‘추가’를 눌러 완료합니다.</li>
          </ol>
        {:else}
          <p>Safari에서는 설치 방법을 확인한 뒤 홈 화면에 추가하세요.</p>
        {/if}
      {/if}
    </div>

    <div class="pwa-actions">
      {#if experience === 'offline'}
        <button type="button" class="primary-action" disabled={retrying} onclick={retryConnection}>
          {retrying ? '확인 중…' : '연결 확인'}
        </button>
      {:else if experience === 'update'}
        <button type="button" class="primary-action" disabled={updating} onclick={applyUpdate}>
          {updating ? '적용 중…' : '지금 업데이트'}
        </button>
      {:else if experience === 'install'}
        <button type="button" class="primary-action" onclick={install}>앱 설치</button>
        <button type="button" class="quiet-action" onclick={dismissInstall}>나중에</button>
      {:else}
        <button
          type="button"
          class="primary-action"
          aria-expanded={iosHelpOpen}
          onclick={() => (iosHelpOpen = !iosHelpOpen)}
        >
          {iosHelpOpen ? '안내 접기' : '설치 방법'}
        </button>
        <button type="button" class="quiet-action" onclick={dismissInstall}>나중에</button>
      {/if}
    </div>
  </section>
{/if}

<style>
  .pwa-notice {
    position: relative;
    z-index: 40;
    display: grid;
    width: auto;
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: 12px;
    align-items: start;
    margin: 10px 14px 0;
    padding: 12px 14px;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--steel), transparent 40%);
    border-radius: 7px;
    background: rgb(13 20 29 / 96%);
    box-shadow: 0 18px 48px rgb(0 0 0 / 46%);
    backdrop-filter: blur(18px);
  }

  .pwa-notice::before {
    position: absolute;
    top: 0;
    right: 0;
    left: 0;
    height: 3px;
    background: var(--steel);
    content: '';
  }

  .pwa-notice.offline::before {
    background: var(--danger);
  }

  .pwa-notice.update::before,
  .pwa-notice.install::before,
  .pwa-notice.ios::before {
    background: var(--brass);
  }

  .pwa-icon {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text-soft);
    background: var(--surface-raised);
  }

  .offline .pwa-icon {
    color: #ef9a9a;
  }

  .update .pwa-icon,
  .install .pwa-icon,
  .ios .pwa-icon {
    color: #e5ca7c;
  }

  .pwa-copy {
    min-width: 0;
  }

  .status-line {
    margin-bottom: 4px;
  }

  .pwa-copy strong {
    display: block;
    color: var(--text);
    font-size: 0.88rem;
    line-height: 1.35;
  }

  .pwa-copy p,
  .pwa-copy ol {
    margin: 5px 0 0;
    color: var(--muted-strong);
    font-size: 0.76rem;
    line-height: 1.45;
  }

  .pwa-copy ol {
    padding-left: 20px;
  }

  .pwa-actions {
    display: flex;
    gap: 6px;
    align-items: center;
    align-self: center;
  }

  .pwa-actions button {
    min-height: 44px;
    padding-inline: 12px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 800;
    white-space: nowrap;
  }

  .pwa-actions button:disabled {
    cursor: wait;
    opacity: 0.65;
  }

  .primary-action {
    color: var(--void);
    background: var(--brass);
  }

  .offline .primary-action {
    color: var(--text);
    background: var(--surface-strong);
  }

  .quiet-action {
    color: var(--muted-strong);
    background: transparent;
  }

  @media (max-width: 719px) {
    .pwa-notice {
      grid-template-columns: auto minmax(0, 1fr);
      gap: 9px 10px;
      margin: 10px max(10px, env(safe-area-inset-right)) 0 max(10px, env(safe-area-inset-left));
      padding: 11px 12px;
      backdrop-filter: none;
    }

    .pwa-actions {
      grid-column: 1 / -1;
      justify-content: flex-end;
    }

    .pwa-actions button {
      flex: 1;
    }

    .pwa-notice.update {
      position: fixed;
      right: max(10px, env(safe-area-inset-right));
      bottom: calc(70px + env(safe-area-inset-bottom));
      left: max(10px, env(safe-area-inset-left));
      z-index: 80;
      display: flex;
      min-height: 56px;
      align-items: center;
      margin: 0;
      padding: 7px 8px 7px 12px;
    }

    .pwa-notice.update .pwa-icon,
    .pwa-notice.update .status-line,
    .pwa-notice.update .pwa-copy p {
      display: none;
    }

    .pwa-notice.update .pwa-copy {
      flex: 1;
    }

    .pwa-notice.update .pwa-actions {
      flex: none;
    }

    .pwa-notice.update .pwa-actions button {
      padding-inline: 11px;
    }
  }

  @media (max-width: 389px) {
    .pwa-icon {
      width: 34px;
      height: 34px;
    }

    .pwa-copy strong {
      font-size: 0.82rem;
    }

    .pwa-copy p,
    .pwa-copy ol {
      font-size: 0.75rem;
    }
  }
</style>
