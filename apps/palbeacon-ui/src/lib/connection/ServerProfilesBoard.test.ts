import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { serverLiveFixture, serverProfilesFixture, sftpSyncFixture } from './connection.fixture';
import ServerProfilesBoard from './ServerProfilesBoard.svelte';
import type {
  NativeServerLiveSnapshot,
  NativeServerProfileMutation,
  NativeServerProfiles,
  NativeSftpSyncStatus,
} from './types';

describe('ServerProfilesBoard', () => {
  it('shows endpoints and only credential presence', async () => {
    render(ServerProfilesBoard, { load: () => Promise.resolve(serverProfilesFixture) });
    expect((await screen.findAllByText('메인 전용 서버')).length).toBeGreaterThan(0);
    expect(screen.getByText('https://192.0.2.10:8212')).toBeInTheDocument();
    expect(screen.getAllByText('Windows에 저장됨')).toHaveLength(2);
    expect(screen.queryByText(/password/i)).not.toBeInTheDocument();
  });

  it('switches the inspector locally without mutating the stored selection', async () => {
    const source = serverProfilesFixture.profiles[0];
    if (source === undefined) throw new Error('The server profile fixture is empty');
    const alternate = {
      ...source,
      id: 'backup-server',
      display_name: '백업 서버',
      host: '192.0.2.20',
    };
    render(ServerProfilesBoard, {
      load: () =>
        Promise.resolve({
          ...serverProfilesFixture,
          profiles: [...serverProfilesFixture.profiles, alternate],
        }),
    });
    await fireEvent.click(await screen.findByRole('button', { name: /백업 서버/ }));
    expect(screen.getByRole('complementary', { name: '선택한 서버 프로필' })).toHaveTextContent(
      '192.0.2.20:8211',
    );
    expect(screen.getByText('사용 중')).toBeInTheDocument();
  });

  it('selects a server through the versioned native mutation boundary', async () => {
    const source = serverProfilesFixture.profiles[0];
    if (source === undefined) throw new Error('The server profile fixture is empty');
    const alternate = { ...source, id: 'backup-server', display_name: '백업 서버' };
    const mutate = vi.fn<(mutation: NativeServerProfileMutation) => Promise<NativeServerProfiles>>(
      () =>
        Promise.resolve({
          ...serverProfilesFixture,
          version: 8,
          selected_profile_id: alternate.id,
          profiles: [source, alternate],
        }),
    );
    render(ServerProfilesBoard, {
      load: () => Promise.resolve({ ...serverProfilesFixture, profiles: [source, alternate] }),
      mutate,
    });

    await fireEvent.click(await screen.findByRole('button', { name: /백업 서버/ }));
    await fireEvent.click(screen.getByRole('button', { name: '이 서버 사용' }));
    expect(mutate).toHaveBeenCalledWith({
      operation: 'select',
      expected_version: 7,
      profile_id: 'backup-server',
    });
    expect(
      await screen.findByText('비밀 값은 화면이나 응답으로 되돌려 보내지 않습니다.'),
    ).toBeInTheDocument();
  });

  it('requires a second explicit action before deleting a profile', async () => {
    const mutate = vi.fn<(mutation: NativeServerProfileMutation) => Promise<NativeServerProfiles>>(
      () =>
        Promise.resolve({
          ...serverProfilesFixture,
          version: 8,
          selected_profile_id: null,
          profiles: [],
        }),
    );
    render(ServerProfilesBoard, {
      load: () => Promise.resolve(serverProfilesFixture),
      mutate,
    });

    await fireEvent.click(await screen.findByRole('button', { name: '삭제' }));
    expect(mutate).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: '삭제 확인' }));
    expect(mutate).toHaveBeenCalledWith({
      operation: 'delete',
      expected_version: 7,
      profile_id: 'main-server',
    });
    expect(await screen.findByText('저장된 서버 없음')).toBeInTheDocument();
  });

  it('requires both risk acknowledgements for the exact HTTP origin', async () => {
    const source = serverProfilesFixture.profiles[0];
    if (source === undefined) throw new Error('The server profile fixture is empty');
    const httpProfile = {
      ...source,
      rest_scheme: 'http' as const,
      has_insecure_rest_http_consent: false,
    };
    const mutate = vi.fn<(mutation: NativeServerProfileMutation) => Promise<NativeServerProfiles>>(
      () =>
        Promise.resolve({
          ...serverProfilesFixture,
          version: 8,
          profiles: [{ ...httpProfile, has_insecure_rest_http_consent: true }],
        }),
    );
    render(ServerProfilesBoard, {
      load: () => Promise.resolve({ ...serverProfilesFixture, profiles: [httpProfile] }),
      mutate,
    });

    await fireEvent.click(await screen.findByRole('button', { name: 'HTTP 위험 검토' }));
    const confirm = screen.getByRole('button', { name: '현재 주소에 동의' });
    expect(confirm).toBeDisabled();
    const acknowledgements = screen.getAllByRole('checkbox');
    expect(acknowledgements).toHaveLength(2);
    await fireEvent.click(acknowledgements[0] as HTMLElement);
    await fireEvent.click(acknowledgements[1] as HTMLElement);
    expect(confirm).toBeEnabled();
    await fireEvent.click(confirm);
    expect(mutate).toHaveBeenCalledWith({
      operation: 'confirm_insecure_rest_http',
      expected_version: 7,
      profile_id: 'main-server',
      confirmed_origin: 'http://192.0.2.10:8212',
      risk_revision: 1,
      accept_plaintext_credentials: true,
      accept_untrusted_responses: true,
    });
  });

  it('loads live REST and SFTP status only after explicit actions', async () => {
    const loadLive = vi.fn<() => Promise<NativeServerLiveSnapshot>>(() =>
      Promise.resolve(serverLiveFixture),
    );
    const loadSftp = vi.fn<() => Promise<NativeSftpSyncStatus>>(() =>
      Promise.resolve(sftpSyncFixture),
    );
    render(ServerProfilesBoard, {
      load: () => Promise.resolve(serverProfilesFixture),
      loadLive,
      loadSftp,
      syncSftp: () => Promise.resolve(sftpSyncFixture),
    });

    expect(loadLive).not.toHaveBeenCalled();
    expect(loadSftp).not.toHaveBeenCalled();
    await fireEvent.click(await screen.findByRole('button', { name: '실시간 상태 조회' }));
    expect(await screen.findByText('공식 REST 상태를 확인했습니다.')).toBeInTheDocument();
    expect(screen.getByText('60')).toBeInTheDocument();
    expect(screen.queryByText('60.0')).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '상태 확인' }));
    expect(await screen.findByText('서버 세이브가 최신 상태입니다.')).toBeInTheDocument();
    expect(screen.queryByText('b0bc52eda611399d11efe26f')).not.toBeInTheDocument();
  });
});
