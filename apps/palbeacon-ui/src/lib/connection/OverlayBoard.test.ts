import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { overlayControlSnapshotFixture } from './connection.fixture';
import OverlayBoard from './OverlayBoard.svelte';
import type { NativeOverlayControlDocument, NativeOverlaySettings } from './types';

const loadFixture = () => Promise.resolve(structuredClone(overlayControlSnapshotFixture));

describe('OverlayBoard', () => {
  it('shows only the minimap decisions and applies an explicit change', async () => {
    const save = vi.fn<
      (version: number, settings: NativeOverlaySettings) => Promise<NativeOverlayControlDocument>
    >((version, settings) =>
      Promise.resolve({
        ...overlayControlSnapshotFixture.control,
        version: version + 1,
        settings,
      }),
    );
    render(OverlayBoard, { load: loadFixture, save });

    const apply = await screen.findByRole('button', { name: '적용' });
    expect(apply).toBeDisabled();
    expect(screen.getByRole('slider', { name: '지도 크기' })).toBeInTheDocument();
    expect(screen.getByRole('slider', { name: '지도 선명도' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '지도 열기' })).toHaveAttribute('href', '/map');
    expect(screen.getByText('PC 지도에서 선택한 항목이 그대로 표시됩니다.')).toBeInTheDocument();

    expect(screen.queryByText('확장 지도')).not.toBeInTheDocument();
    expect(screen.queryByText(/FPS/)).not.toBeInTheDocument();
    expect(screen.queryByText('입력 잠금')).not.toBeInTheDocument();
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    expect(screen.queryByText(/\d+px/)).not.toBeInTheDocument();
    expect(screen.queryByText(/\d+%/)).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('radio', { name: /진행 방향/ }));
    await waitFor(() => expect(apply).toBeEnabled());
    await fireEvent.click(apply);

    await waitFor(() => expect(save).toHaveBeenCalledOnce());
    expect(save.mock.calls[0]?.[0]).toBe(9);
    expect(save.mock.calls[0]?.[1].rotation_mode).toBe('heading_up');
    expect(apply).toBeDisabled();
  });

  it('captures a modifier chord for the direction toggle', async () => {
    const save = vi.fn<
      (version: number, settings: NativeOverlaySettings) => Promise<NativeOverlayControlDocument>
    >((version, settings) =>
      Promise.resolve({
        ...overlayControlSnapshotFixture.control,
        version: version + 1,
        settings,
      }),
    );
    render(OverlayBoard, { load: loadFixture, save });

    const changeButtons = await screen.findAllByRole('button', { name: '변경' });
    await fireEvent.click(changeButtons[1]!);
    expect(screen.getByRole('dialog', { name: '지도 방향 단축키 변경' })).toBeInTheDocument();

    await fireEvent.keyDown(window, {
      key: 'K',
      code: 'KeyK',
      ctrlKey: true,
      shiftKey: true,
    });

    expect(screen.queryByRole('dialog', { name: '지도 방향 단축키 변경' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('Ctrl + Shift + K')).toBeInTheDocument();

    const apply = screen.getByRole('button', { name: '적용' });
    await fireEvent.click(apply);
    await waitFor(() => expect(save).toHaveBeenCalledOnce());
    expect(save.mock.calls[0]?.[1].hotkey_bindings.rotation_toggle).toEqual({
      modifiers: { control: true, alt: false, shift: true, windows: false },
      virtual_key: 0x4b,
    });
  });

  it('rejects unsafe and duplicate hotkeys without exposing native codes', async () => {
    render(OverlayBoard, { load: loadFixture });

    const changeButtons = await screen.findAllByRole('button', { name: '변경' });
    await fireEvent.click(changeButtons[1]!);
    await fireEvent.keyDown(window, { key: 'K', code: 'KeyK' });
    expect(screen.getByRole('alert')).toHaveTextContent(
      '문자와 숫자 키는 Ctrl, Alt, Shift 또는 Win과 함께 사용하세요.',
    );

    await fireEvent.keyDown(window, { key: 'F8', code: 'F8' });
    expect(screen.getByRole('alert')).toHaveTextContent('이미 다른 단축키에서 사용 중입니다.');
    expect(screen.queryByText(/virtual_key|0x77|119/)).not.toBeInTheDocument();
  });

  it('hides native errors and offers a plain retry action', async () => {
    render(OverlayBoard, {
      load: loadFixture,
      save: () =>
        Promise.reject(new Error('native version=9 conflict at C:\\private\\overlay.json')),
    });

    await screen.findByText('F8');
    await fireEvent.click(screen.getByRole('switch', { name: '오버레이 표시' }));
    await fireEvent.click(screen.getByRole('button', { name: '적용' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('오버레이 설정을 적용하지 못했습니다.');
    expect(alert).not.toHaveTextContent('version=9');
    expect(alert).not.toHaveTextContent('overlay.json');
    expect(screen.getByRole('button', { name: '다시 시도' })).toBeInTheDocument();
  });

  it('does not report native float serialization noise as a user change', async () => {
    const snapshot = structuredClone(overlayControlSnapshotFixture);
    snapshot.control.settings.opacity = 0.9200000166893005;
    snapshot.control.settings.zoom = 1.75;
    snapshot.control.settings.rotation_mode = 'heading_up';
    snapshot.control.settings.diameter_px = 360;
    snapshot.control.settings.poi_filters.enabled_layer_ids = ['ancient-bark', 'ore-coal', 'tower'];
    snapshot.control.settings.poi_filters.selected_pal_ids = ['BOSS_JetDragon', 'SkyDragon'];
    snapshot.control.settings.hotkey_bindings.overlay_visibility = {
      modifiers: { control: true, alt: false, shift: true, windows: false },
      virtual_key: 0x77,
    };

    render(OverlayBoard, { load: () => Promise.resolve(snapshot) });

    const apply = await screen.findByRole('button', { name: '적용' });
    await waitFor(() => expect(apply).toBeDisabled());
  });
});
