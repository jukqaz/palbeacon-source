import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { appSettingsFixture } from './connection.fixture';
import type { NativeAppSettings, NativeAppSettingsDocument } from './types';
import SettingsBoard from './SettingsBoard.svelte';

describe('SettingsBoard', () => {
  it('keeps save disabled until a setting changes and saves with the current version', async () => {
    const save = vi.fn<
      (version: number, settings: NativeAppSettings) => Promise<NativeAppSettingsDocument>
    >((version, settings) =>
      Promise.resolve({ ...appSettingsFixture, version: version + 1, settings }),
    );
    render(SettingsBoard, {
      load: () => Promise.resolve(appSettingsFixture),
      save,
      loadAutostart: () => Promise.resolve(true),
      setAutostart: () => Promise.resolve(),
    });

    await screen.findByRole('switch', { name: /수동 시작 시 앱 창 열기/ });
    expect(screen.queryByRole('button', { name: '설정 저장' })).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('switch', { name: /수동 시작 시 앱 창 열기/ }));
    const submit = screen.getByRole('button', { name: '설정 저장' });
    expect(submit).toBeEnabled();
    await fireEvent.click(submit);

    expect(save).toHaveBeenCalledWith(4, {
      ...appSettingsFixture.settings,
      open_app_on_manual_start: true,
    });
    expect(screen.queryByRole('button', { name: '설정 저장' })).not.toBeInTheDocument();
  });

  it('keeps save and character selection in the dedicated personal data screen', async () => {
    render(SettingsBoard, {
      load: () => Promise.resolve(appSettingsFixture),
      save: () => Promise.resolve(appSettingsFixture),
      loadAutostart: () => Promise.resolve(true),
      setAutostart: () => Promise.resolve(),
    });

    await screen.findByRole('heading', { name: '시작 동작' });
    expect(screen.queryByText('데이터 선택')).not.toBeInTheDocument();
    expect(screen.queryByText('세이브 가져오기')).not.toBeInTheDocument();
    expect(screen.queryByText('캐릭터')).not.toBeInTheDocument();
  });

  it('shows an explicit recovery action after a version conflict', async () => {
    render(SettingsBoard, {
      load: () => Promise.resolve(appSettingsFixture),
      save: () => Promise.reject(new Error('설정이 다른 창에서 변경되었습니다')),
      loadAutostart: () => Promise.resolve(true),
      setAutostart: () => Promise.resolve(),
    });
    await fireEvent.click(await screen.findByRole('switch', { name: /수동 시작 시 앱 창 열기/ }));
    await fireEvent.click(screen.getByRole('button', { name: '설정 저장' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('설정을 저장하지 못했습니다.');
    expect(alert).not.toHaveTextContent('설정이 다른 창에서 변경되었습니다');
    expect(screen.getByRole('button', { name: '최신 설정 다시 읽기' })).toBeInTheDocument();
  });

  it('updates autostart through the official plugin without duplicating it in app settings', async () => {
    const save =
      vi.fn<(version: number, settings: NativeAppSettings) => Promise<NativeAppSettingsDocument>>();
    const setAutostart = vi.fn<(enabled: boolean) => Promise<void>>(() => Promise.resolve());
    render(SettingsBoard, {
      load: () => Promise.resolve(appSettingsFixture),
      save,
      loadAutostart: () => Promise.resolve(true),
      setAutostart,
    });

    await fireEvent.click(await screen.findByRole('switch', { name: /Windows 로그인 시 실행/ }));
    await fireEvent.click(screen.getByRole('button', { name: '설정 저장' }));

    expect(setAutostart).toHaveBeenCalledWith(false);
    expect(save).not.toHaveBeenCalled();
  });
});
