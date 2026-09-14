import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ConnectionProfileBoard from './ConnectionProfileBoard.svelte';
import {
  nativeProfileFixture,
  saveSourceCandidateFixture,
  saveSourceProbeFixture,
  stagedSaveFixture,
} from './connection.fixture';
import type { NativeSaveSourceProbe, NativeStagedSave } from './types';

describe('ConnectionProfileBoard', () => {
  it('shows only the protected-save task without repeating unrelated runtime status', async () => {
    render(ConnectionProfileBoard, { load: () => Promise.resolve(nativeProfileFixture) });
    expect(await screen.findByRole('heading', { name: '내 데이터' })).toBeInTheDocument();
    expect(screen.queryByText('현재 연결')).not.toBeInTheDocument();
    expect(
      screen.queryByText('현재 위치를 사용할 수 없어 지도만 표시합니다.'),
    ).not.toBeInTheDocument();
    expect(screen.queryByText('앱 연결')).not.toBeInTheDocument();
    expect(screen.queryByText('표시 안 함')).not.toBeInTheDocument();
    expect(screen.queryByText('보호 복사본')).not.toBeInTheDocument();
    expect(screen.queryByText('DedicatedServerWorld')).not.toBeInTheDocument();
    expect(screen.queryByText('게임 빌드')).not.toBeInTheDocument();
    expect(screen.queryByText('지도 빌드')).not.toBeInTheDocument();
    expect(screen.queryByText('검증 지도 팩')).not.toBeInTheDocument();
    expect(screen.queryByText('좌표 대기')).not.toBeInTheDocument();
    expect(await screen.findByText('17.5 MB')).toBeInTheDocument();
    expect(screen.queryByText('approved_map_pack')).not.toBeInTheDocument();
    expect(screen.queryByText('b0bc52eda611399d11efe26f')).not.toBeInTheDocument();
  });

  it('keeps native failures recoverable', async () => {
    render(ConnectionProfileBoard, {
      load: () => Promise.reject(new Error('Core 연결 실패')),
    });
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Windows 상태를 불러오지 못했습니다.');
    expect(alert).not.toHaveTextContent('Core 연결 실패');
    expect(screen.getByRole('button', { name: '다시 확인' })).toBeInTheDocument();
  });

  it('keeps the current state when the native folder picker is cancelled', async () => {
    const probeSource = vi.fn<(sourceRoot: string) => Promise<NativeSaveSourceProbe>>();
    render(ConnectionProfileBoard, {
      load: () => Promise.resolve(structuredClone(nativeProfileFixture)),
      selectSource: () => Promise.resolve(null),
      probeSource,
    });

    const findSave = await screen.findByRole('button', { name: '세이브 찾기' });
    await fireEvent.click(findSave);

    expect(probeSource).not.toHaveBeenCalled();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('stages only the single world returned by an explicit successful probe', async () => {
    const probeSource = vi
      .fn<(sourceRoot: string) => Promise<NativeSaveSourceProbe>>()
      .mockResolvedValue(saveSourceProbeFixture);
    const stageSource = vi
      .fn<(worldRoot: string) => Promise<NativeStagedSave>>()
      .mockResolvedValue({
        ...stagedSaveFixture,
        import_id: 'c0bc52eda611399d11efe26f',
      });
    render(ConnectionProfileBoard, {
      load: () => Promise.resolve(structuredClone(nativeProfileFixture)),
      selectSource: () => Promise.resolve('C:\\Palworld\\Saved\\SaveGames'),
      probeSource,
      stageSource,
    });

    const findSave = await screen.findByRole('button', { name: '세이브 찾기' });
    expect(screen.queryByRole('button', { name: '보호 복사 만들기' })).not.toBeInTheDocument();
    expect(stageSource).not.toHaveBeenCalled();

    expect(screen.queryByText('C:\\Palworld\\Saved\\SaveGames')).not.toBeInTheDocument();
    await fireEvent.click(findSave);

    expect(await screen.findByText('보호 복사 가능')).toBeInTheDocument();
    expect(probeSource).toHaveBeenCalledWith('C:\\Palworld\\Saved\\SaveGames');
    expect(screen.queryByText(/DedicatedServerWorld/u)).not.toBeInTheDocument();
    const stageButton = screen.getByRole('button', { name: '보호 복사 만들기' });
    expect(stageButton).toBeEnabled();

    await fireEvent.click(stageButton);
    expect(stageSource).toHaveBeenCalledWith(
      'C:\\Palworld\\Saved\\SaveGames\\0\\DedicatedServerWorld',
    );
    expect(await screen.findByRole('status')).toHaveTextContent('원본을 변경하지 않고');
    expect(screen.queryByText('c0bc52eda611399d11efe26f')).not.toBeInTheDocument();
  });

  it('does not stage ambiguous save roots', async () => {
    const stageSource = vi.fn<(worldRoot: string) => Promise<NativeStagedSave>>();
    render(ConnectionProfileBoard, {
      load: () => Promise.resolve(structuredClone(nativeProfileFixture)),
      selectSource: () => Promise.resolve('C:\\Palworld\\Saved\\SaveGames'),
      probeSource: () =>
        Promise.resolve({
          ...saveSourceProbeFixture,
          status: 'multiple_save_worlds',
          candidates: [
            saveSourceCandidateFixture,
            {
              ...saveSourceCandidateFixture,
              world_root: 'C:\\Palworld\\Saved\\SaveGames\\0\\OtherWorld',
              world_folder_name: 'OtherWorld',
            },
          ],
        }),
      stageSource,
    });

    const findSave = await screen.findByRole('button', { name: '세이브 찾기' });
    await fireEvent.click(findSave);

    expect(await screen.findByText('월드를 하나로 좁혀 주세요')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '보호 복사 만들기' })).toBeDisabled();
    expect(stageSource).not.toHaveBeenCalled();
  });
});
