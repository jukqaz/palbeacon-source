import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import PwaExperience from './PwaExperience.svelte';

class TestInstallPromptEvent extends Event {
  prompt = vi.fn<() => Promise<void>>(async () => undefined);
  userChoice = Promise.resolve({ outcome: 'accepted' as const, platform: 'web' });
}

const setOnline = (value: boolean) => {
  Object.defineProperty(navigator, 'onLine', {
    configurable: true,
    value,
  });
};

describe('PWA experience', () => {
  afterEach(() => {
    setOnline(true);
    sessionStorage.clear();
  });

  it('explains the offline state without hiding the current screen', async () => {
    setOnline(false);
    render(PwaExperience);

    expect(await screen.findByText('연결 없이 저장된 데이터를 보고 있습니다.')).toBeVisible();
    expect(screen.getByRole('button', { name: '연결 확인' })).toHaveStyle({ minHeight: '44px' });
  });

  it('offers the browser install prompt and remembers a session dismissal', async () => {
    render(PwaExperience);
    const prompt = new TestInstallPromptEvent('beforeinstallprompt', { cancelable: true });
    window.dispatchEvent(prompt);

    const install = await screen.findByRole('button', { name: '앱 설치' });
    await fireEvent.click(install);
    expect(prompt.prompt).toHaveBeenCalledOnce();

    const secondPrompt = new TestInstallPromptEvent('beforeinstallprompt', { cancelable: true });
    window.dispatchEvent(secondPrompt);
    await fireEvent.click(await screen.findByRole('button', { name: '나중에' }));

    await waitFor(() => expect(screen.queryByTestId('pwa-experience')).not.toBeInTheDocument());
    expect(sessionStorage.getItem('palbeacon:pwa-install-dismissed')).toBe('true');
  });
});
