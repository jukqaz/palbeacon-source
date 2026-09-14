<script lang="ts">
  import { onMount } from 'svelte';
  import { formatUserNumber } from '$lib/shared/format/user-number';
  import PalCheckbox from '$lib/shared/ui/PalCheckbox.svelte';
  import {
    loadNativeServerLive,
    loadNativeServerProfiles,
    loadNativeSftpStatus,
    mutateNativeServerProfiles,
    syncNativeSftp,
  } from './native';
  import type {
    NativeServerProfile,
    NativeServerProfileInput,
    NativeServerProfileMutation,
    NativeServerLiveSnapshot,
    NativeServerProfiles,
    NativeSftpSyncStatus,
  } from './types';

  interface Props {
    load?: () => Promise<NativeServerProfiles>;
    mutate?: (request: NativeServerProfileMutation) => Promise<NativeServerProfiles>;
    loadLive?: (profileId: string) => Promise<NativeServerLiveSnapshot>;
    loadSftp?: (profileId: string) => Promise<NativeSftpSyncStatus | null>;
    syncSftp?: (profileId: string) => Promise<NativeSftpSyncStatus>;
  }

  let {
    load = loadNativeServerProfiles,
    mutate = mutateNativeServerProfiles,
    loadLive = loadNativeServerLive,
    loadSftp = loadNativeSftpStatus,
    syncSftp = syncNativeSftp,
  }: Props = $props();
  let document = $state<NativeServerProfiles | null>(null);
  let selectedId = $state<string | null>(null);
  let editor = $state<'create' | 'edit' | null>(null);
  let draft = $state<NativeServerProfileInput | null>(null);
  let sftpPassword = $state('');
  let restPassword = $state('');
  let confirmDelete = $state(false);
  let consentOpen = $state(false);
  let acceptPlaintextCredentials = $state(false);
  let acceptUntrustedResponses = $state(false);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let liveBusy = $state(false);
  let sftpBusy = $state(false);
  let liveSnapshot = $state<NativeServerLiveSnapshot | null>(null);
  let sftpStatus = $state<NativeSftpSyncStatus | null>(null);
  let sftpChecked = $state(false);

  const selected = $derived(
    document?.profiles.find((profile) => profile.id === selectedId) ??
      document?.profiles[0] ??
      null,
  );

  const emptyProfile = (): NativeServerProfileInput => ({
    id: `server-${crypto.randomUUID()}`,
    display_name: '',
    host: '',
    rest_host: null,
    game_port: 8211,
    query_port: 27015,
    sftp_port: 22,
    rest_port: 8212,
    rcon_port: null,
    rest_scheme: 'https',
    rest_username: null,
    sftp_username: null,
    save_root: null,
    ssh_host_key_fingerprint: null,
  });

  const profileInput = (profile: NativeServerProfile): NativeServerProfileInput => ({
    id: profile.id,
    display_name: profile.display_name,
    host: profile.host,
    rest_host: profile.rest_host,
    game_port: profile.game_port,
    query_port: profile.query_port,
    sftp_port: profile.sftp_port,
    rest_port: profile.rest_port,
    rcon_port: profile.rcon_port,
    rest_scheme: profile.rest_scheme,
    rest_username: profile.rest_username,
    sftp_username: profile.sftp_username,
    save_root: profile.save_root,
    ssh_host_key_fingerprint: profile.ssh_host_key_fingerprint,
  });

  const refresh = async () => {
    loading = true;
    error = null;
    try {
      document = await load();
      selectedId = document.selected_profile_id ?? document.profiles[0]?.id ?? null;
      editor = null;
    } catch (reason) {
      console.error('서버 프로필을 불러오지 못했습니다.', reason);
      error = '서버 프로필을 불러오지 못했습니다.';
    } finally {
      loading = false;
    }
  };

  const applyMutation = async (request: NativeServerProfileMutation) => {
    busy = true;
    error = null;
    try {
      document = await mutate(request);
      selectedId =
        document.selected_profile_id ??
        document.profiles.find((profile) => profile.id === selectedId)?.id ??
        document.profiles[0]?.id ??
        null;
      editor = null;
      draft = null;
      sftpPassword = '';
      restPassword = '';
      confirmDelete = false;
      consentOpen = false;
      acceptPlaintextCredentials = false;
      acceptUntrustedResponses = false;
      liveSnapshot = null;
      sftpStatus = null;
      sftpChecked = false;
    } catch (reason) {
      console.error('서버 프로필 변경을 저장하지 못했습니다.', reason);
      error = '서버 프로필 변경을 저장하지 못했습니다.';
    } finally {
      busy = false;
    }
  };

  const beginCreate = () => {
    editor = 'create';
    draft = emptyProfile();
    sftpPassword = '';
    restPassword = '';
    confirmDelete = false;
  };
  const beginEdit = () => {
    if (!selected) return;
    editor = 'edit';
    draft = profileInput(selected);
    sftpPassword = '';
    restPassword = '';
    confirmDelete = false;
  };
  const submitProfile = () => {
    if (!document || !draft) return;
    void applyMutation({
      operation: 'upsert',
      expected_version: document.version,
      profile: { ...draft },
      sftp_password: sftpPassword.trim() || null,
      rest_password: restPassword.trim() || null,
    });
  };
  const selectCurrent = () => {
    if (!document || !selected) return;
    void applyMutation({
      operation: 'select',
      expected_version: document.version,
      profile_id: selected.id,
    });
  };
  const deleteCurrent = () => {
    if (!document || !selected) return;
    void applyMutation({
      operation: 'delete',
      expected_version: document.version,
      profile_id: selected.id,
    });
  };
  const httpOrigin = (profile: NativeServerProfile): string | null => {
    if (profile.rest_port === null) return null;
    const host = profile.rest_host ?? profile.host;
    const authority = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host;
    return `http://${authority}:${profile.rest_port.toString()}`;
  };
  const confirmHttp = () => {
    if (!document || !selected) return;
    const origin = httpOrigin(selected);
    if (!origin) return;
    void applyMutation({
      operation: 'confirm_insecure_rest_http',
      expected_version: document.version,
      profile_id: selected.id,
      confirmed_origin: origin,
      risk_revision: 1,
      accept_plaintext_credentials: acceptPlaintextCredentials,
      accept_untrusted_responses: acceptUntrustedResponses,
    });
  };
  const revokeHttp = () => {
    if (!document || !selected) return;
    void applyMutation({
      operation: 'revoke_insecure_rest_http',
      expected_version: document.version,
      profile_id: selected.id,
    });
  };
  const checkLive = async () => {
    if (!selected) return;
    liveBusy = true;
    error = null;
    try {
      liveSnapshot = await loadLive(selected.id);
    } catch (reason) {
      console.error('서버 실시간 상태를 확인하지 못했습니다.', reason);
      error = '서버 실시간 상태를 확인하지 못했습니다.';
    } finally {
      liveBusy = false;
    }
  };
  const checkSftp = async () => {
    if (!selected) return;
    sftpBusy = true;
    error = null;
    try {
      sftpStatus = await loadSftp(selected.id);
      sftpChecked = true;
    } catch (reason) {
      console.error('SFTP 동기화 상태를 확인하지 못했습니다.', reason);
      error = '동기화 상태를 확인하지 못했습니다.';
    } finally {
      sftpBusy = false;
    }
  };
  const runSftp = async () => {
    if (!selected) return;
    sftpBusy = true;
    error = null;
    try {
      sftpStatus = await syncSftp(selected.id);
      sftpChecked = true;
    } catch (reason) {
      console.error('SFTP 보호 복사를 완료하지 못했습니다.', reason);
      error = '보호 복사를 완료하지 못했습니다.';
    } finally {
      sftpBusy = false;
    }
  };
  const endpoint = (profile: NativeServerProfile, port: number | null): string =>
    port === null ? '설정 안 됨' : `${profile.host}:${port.toString()}`;

  onMount(() => {
    void refresh();
  });
</script>

<section class="server-screen pal-screen">
  <header class="page-head pal-page-head">
    <div>
      <span class="pal-page-kicker">WINDOWS · CREDENTIAL SAFE</span>
      <h1>서버 연결</h1>
      <p>프로필은 로컬 파일, 비밀번호는 Windows 자격 증명 저장소에 분리해 보관합니다.</p>
    </div>
    <div class="head-actions">
      <button type="button" disabled={loading || busy} onclick={beginCreate}>서버 추가</button>
      <button type="button" disabled={loading || busy} onclick={() => void refresh()}
        >{loading ? '확인 중' : '새로고침'}</button
      >
    </div>
  </header>

  {#if error}
    <section class="state error" role="alert">
      <strong>서버 프로필 변경을 완료하지 못했습니다.</strong>
      <p>{error}</p>
      <button type="button" disabled={busy} onclick={() => void refresh()}
        >최신 목록 다시 읽기</button
      >
    </section>
  {/if}

  {#if document}
    <div class="server-layout">
      <section class="profile-list">
        <header><span>서버</span><strong>{document.profiles.length}개</strong></header>
        {#if document.profiles.length === 0}
          <div class="empty">
            <strong>저장된 서버 없음</strong>
            <p>서버 추가로 첫 연결 정보를 등록할 수 있습니다.</p>
          </div>
        {:else}
          <div class="profiles">
            {#each document.profiles as profile (profile.id)}
              <button
                type="button"
                class:active={selected?.id === profile.id}
                aria-pressed={selected?.id === profile.id}
                disabled={busy}
                onclick={() => {
                  selectedId = profile.id;
                  editor = null;
                  confirmDelete = false;
                  liveSnapshot = null;
                  sftpStatus = null;
                  sftpChecked = false;
                }}
              >
                <span>{profile.display_name}</span><small>{profile.host}:{profile.game_port}</small>
                {#if document.selected_profile_id === profile.id}<em>사용 중</em>{/if}
              </button>
            {/each}
          </div>
        {/if}
      </section>

      <aside class="inspector" aria-label={editor ? '서버 프로필 편집' : '선택한 서버 프로필'}>
        <header>
          <strong
            >{editor === 'create'
              ? '서버 추가'
              : editor === 'edit'
                ? '서버 편집'
                : selected
                  ? '서버 정보'
                  : '서버 선택'}</strong
          >
        </header>
        {#if editor && draft}
          <form
            class="profile-form"
            onsubmit={(event) => {
              event.preventDefault();
              submitProfile();
            }}
          >
            <div class="field-grid">
              <label
                ><span>표시 이름</span><input
                  required
                  maxlength="64"
                  bind:value={draft.display_name}
                /></label
              >
              <label><span>게임 호스트</span><input required bind:value={draft.host} /></label>
              <label
                ><span>게임 포트</span><input
                  required
                  type="number"
                  min="1"
                  max="65535"
                  bind:value={draft.game_port}
                /></label
              >
              <label
                ><span>Query 포트</span><input
                  type="number"
                  min="1"
                  max="65535"
                  bind:value={draft.query_port}
                /></label
              >
              <label
                ><span>SFTP 포트</span><input
                  type="number"
                  min="1"
                  max="65535"
                  bind:value={draft.sftp_port}
                /></label
              >
              <label
                ><span>REST 방식</span><select bind:value={draft.rest_scheme}
                  ><option value="https">HTTPS</option><option value="http">HTTP</option></select
                ></label
              >
              <label
                ><span>REST 호스트</span><input
                  placeholder="비우면 게임 호스트"
                  bind:value={draft.rest_host}
                /></label
              >
              <label
                ><span>REST 포트</span><input
                  type="number"
                  min="1"
                  max="65535"
                  bind:value={draft.rest_port}
                /></label
              >
              <label
                ><span>REST 사용자</span><input
                  autocomplete="username"
                  bind:value={draft.rest_username}
                /></label
              >
              <label
                ><span>SFTP 사용자</span><input
                  autocomplete="username"
                  bind:value={draft.sftp_username}
                /></label
              >
              <label><span>원격 세이브 경로</span><input bind:value={draft.save_root} /></label>
              <label class="wide"
                ><span>SSH 호스트 키 지문</span><input
                  placeholder="SHA256:..."
                  bind:value={draft.ssh_host_key_fingerprint}
                /></label
              >
              <label
                ><span>SFTP 새 비밀번호</span><input
                  type="password"
                  autocomplete="new-password"
                  placeholder="비우면 유지"
                  bind:value={sftpPassword}
                /></label
              >
              <label
                ><span>REST 새 비밀번호</span><input
                  type="password"
                  autocomplete="new-password"
                  placeholder="비우면 유지"
                  bind:value={restPassword}
                /></label
              >
            </div>
            {#if draft.rest_scheme === 'http'}
              <p class="http-warning" role="status">
                HTTP는 자격 증명이 암호화되지 않습니다. 저장 후 현재 숫자 IP와 포트에 별도 동의해야
                연결됩니다.
              </p>
            {/if}
            <footer class="editor-actions">
              <button
                type="button"
                disabled={busy}
                onclick={() => {
                  editor = null;
                  draft = null;
                }}>취소</button
              >
              <button class="primary-action" type="submit" disabled={busy}
                >{busy ? '저장 중' : '프로필 저장'}</button
              >
            </footer>
          </form>
        {:else if selected}
          <div class="identity">
            <h2>{selected.display_name}</h2>
          </div>
          <dl>
            <div>
              <dt>게임</dt>
              <dd>{endpoint(selected, selected.game_port)}</dd>
            </div>
            <div>
              <dt>Query</dt>
              <dd>{endpoint(selected, selected.query_port)}</dd>
            </div>
            <div>
              <dt>REST</dt>
              <dd>
                {selected.rest_port === null
                  ? '설정 안 됨'
                  : `${selected.rest_scheme}://${selected.rest_host ?? selected.host}:${selected.rest_port.toString()}`}
              </dd>
            </div>
            <div>
              <dt>SFTP</dt>
              <dd>{endpoint(selected, selected.sftp_port)}</dd>
            </div>
            <div>
              <dt>REST 자격 증명</dt>
              <dd>{selected.has_rest_password ? 'Windows에 저장됨' : '없음'}</dd>
            </div>
            <div>
              <dt>SFTP 자격 증명</dt>
              <dd>{selected.has_sftp_password ? 'Windows에 저장됨' : '없음'}</dd>
            </div>
            <div>
              <dt>세이브 경로</dt>
              <dd>{selected.save_root ?? '설정 안 됨'}</dd>
            </div>
            <div>
              <dt>HTTP 동의</dt>
              <dd>{selected.has_insecure_rest_http_consent ? '현재 주소에 동의됨' : '없음'}</dd>
            </div>
          </dl>
          {#if selected.rest_scheme === 'http' && selected.rest_port !== null}
            <section class="consent-panel" aria-label="평문 HTTP 위험 동의">
              <header><span>보안 확인</span><strong>{httpOrigin(selected)}</strong></header>
              {#if selected.has_insecure_rest_http_consent}
                <p>
                  현재 숫자 IP와 포트에만 동의가 적용되어 있습니다. 주소가 바뀌면 자동으로
                  무효화됩니다.
                </p>
                <button type="button" disabled={busy} onclick={revokeHttp}>HTTP 동의 철회</button>
              {:else if consentOpen}
                <p>HTTPS 없이 전송하면 비밀번호가 노출되고 응답이 변조될 수 있습니다.</p>
                <div class="consent-option">
                  <PalCheckbox
                    bind:checked={acceptPlaintextCredentials}
                    label="자격 증명이 평문으로 노출될 수 있음을 이해했습니다."
                  />
                  <span>자격 증명이 평문으로 노출될 수 있음을 이해했습니다.</span>
                </div>
                <div class="consent-option">
                  <PalCheckbox
                    bind:checked={acceptUntrustedResponses}
                    label="서버 응답을 신뢰할 수 없음을 이해했습니다."
                  />
                  <span>서버 응답을 신뢰할 수 없음을 이해했습니다.</span>
                </div>
                <div class="consent-actions">
                  <button type="button" onclick={() => (consentOpen = false)}>취소</button>
                  <button
                    class="danger-action"
                    type="button"
                    disabled={busy || !acceptPlaintextCredentials || !acceptUntrustedResponses}
                    onclick={confirmHttp}>현재 주소에 동의</button
                  >
                </div>
              {:else}
                <p>REST 기능은 현재 주소에 대한 명시적 위험 동의 전까지 차단됩니다.</p>
                <button type="button" onclick={() => (consentOpen = true)}>HTTP 위험 검토</button>
              {/if}
            </section>
          {/if}
          <section class="operations-panel" aria-label="서버 읽기 작업">
            <article>
              <header>
                <span>서버 상태</span><strong>{liveSnapshot ? '조회 결과' : '조회 전'}</strong>
              </header>
              <p>{liveSnapshot?.message_ko ?? '요청할 때만 공식 REST 상태를 읽습니다.'}</p>
              {#if liveSnapshot?.metrics.value}
                <dl class="metric-row">
                  {#if liveSnapshot.metrics.value.server_fps != null}
                    <div>
                      <dt>FPS</dt>
                      <dd>
                        {formatUserNumber(liveSnapshot.metrics.value.server_fps, {
                          maximumFractionDigits: 1,
                        })}
                      </dd>
                    </div>
                  {/if}
                  {#if liveSnapshot.metrics.value.current_player_count != null && liveSnapshot.metrics.value.max_player_count != null}
                    <div>
                      <dt>접속자</dt>
                      <dd>
                        {formatUserNumber(liveSnapshot.metrics.value.current_player_count, {
                          maximumFractionDigits: 0,
                        })} / {formatUserNumber(liveSnapshot.metrics.value.max_player_count, {
                          maximumFractionDigits: 0,
                        })}
                      </dd>
                    </div>
                  {/if}
                </dl>
              {/if}
              <button type="button" disabled={liveBusy || busy} onclick={() => void checkLive()}
                >{liveBusy ? '조회 중' : '실시간 상태 조회'}</button
              >
            </article>
            <article>
              <header>
                <span>세이브 동기화</span><strong
                  >{sftpStatus || sftpChecked ? '확인 결과' : '확인 전'}</strong
                >
              </header>
              <p>
                {sftpStatus?.message_ko ??
                  (sftpChecked
                    ? '아직 SFTP 동기화 기록이 없습니다.'
                    : '원격 원본은 읽기만 하고 로컬 보호 복사본을 만듭니다.')}
              </p>
              <div class="operation-actions">
                <button type="button" disabled={sftpBusy || busy} onclick={() => void checkSftp()}
                  >상태 확인</button
                >
                <button type="button" disabled={sftpBusy || busy} onclick={() => void runSftp()}
                  >{sftpBusy ? '동기화 중' : '보호 복사 동기화'}</button
                >
              </div>
            </article>
          </section>
          <footer class="inspector-actions">
            <button
              type="button"
              disabled={busy || document.selected_profile_id === selected.id}
              onclick={selectCurrent}>이 서버 사용</button
            >
            <button type="button" disabled={busy} onclick={beginEdit}>편집</button>
            {#if confirmDelete}<button
                class="danger-action"
                type="button"
                disabled={busy}
                onclick={deleteCurrent}>삭제 확인</button
              >
            {:else}<button type="button" disabled={busy} onclick={() => (confirmDelete = true)}
                >삭제</button
              >{/if}
          </footer>
          <p class="credential-note">비밀 값은 화면이나 응답으로 되돌려 보내지 않습니다.</p>
        {:else}
          <div class="empty">
            <strong>선택할 서버 없음</strong>
            <p>서버 추가로 시작할 수 있습니다.</p>
          </div>
        {/if}
      </aside>
    </div>
  {:else if loading}
    <section class="state" aria-live="polite">
      <strong>서버 프로필을 읽는 중입니다.</strong>
    </section>
  {/if}
</section>

<style>
  section > header span {
    color: var(--brass);
    font-size: 0.75rem;
    font-weight: 850;
    letter-spacing: 0.13em;
  }
  h2 {
    margin: 0;
  }
  .empty p,
  .state p,
  .credential-note {
    margin: 0;
    color: var(--muted-strong);
    font-size: 0.75rem;
    line-height: 1.7;
  }
  .head-actions,
  .inspector-actions,
  .editor-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  button {
    min-height: 42px;
    padding: 0 16px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text);
    background: var(--surface-raised);
    cursor: pointer;
    font-weight: 800;
  }
  button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
  .primary-action {
    border-color: color-mix(in srgb, var(--accent), transparent 20%);
    background: var(--accent);
  }
  .danger-action {
    border-color: var(--danger);
    color: var(--danger);
  }
  .server-layout {
    display: grid;
    grid-template-columns: minmax(260px, 0.62fr) minmax(520px, 1.38fr);
    gap: 12px;
    margin-top: 12px;
  }
  .profile-list,
  .inspector,
  .state {
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--ink);
  }
  section > header,
  aside > header {
    display: flex;
    min-height: 56px;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
  }
  .profiles {
    display: grid;
    gap: 5px;
    padding: 10px;
  }
  .profiles button {
    position: relative;
    display: grid;
    min-height: 66px;
    align-content: center;
    justify-items: start;
    padding: 10px 88px 10px 12px;
    text-align: left;
  }
  .profiles button.active {
    border-color: var(--accent);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .profiles small {
    margin-top: 4px;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .profiles em {
    position: absolute;
    top: 10px;
    right: 10px;
    color: var(--success);
    font-size: 0.75rem;
    font-style: normal;
  }
  .identity {
    display: grid;
    gap: 5px;
    padding: 18px;
    border-bottom: 1px solid var(--border);
  }
  dl,
  .field-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin: 0;
  }
  dl div {
    min-width: 0;
    padding: 13px 16px;
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  dt,
  .field-grid label > span {
    color: var(--muted);
    font-size: 0.75rem;
  }
  dd {
    margin: 5px 0 0;
    overflow-wrap: anywhere;
    font-size: 0.76rem;
    font-weight: 800;
  }
  .field-grid {
    gap: 10px;
    padding: 14px;
  }
  .field-grid label {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .field-grid .wide {
    grid-column: 1 / -1;
  }
  input,
  select {
    min-width: 0;
    min-height: 40px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--text);
    background: var(--void);
    font-size: 0.75rem;
  }
  input:disabled {
    color: var(--muted);
  }
  .http-warning {
    margin: 0 14px 14px;
    padding: 12px;
    border: 1px solid color-mix(in srgb, var(--brass), transparent 40%);
    color: var(--brass);
    font-size: 0.75rem;
    line-height: 1.6;
  }
  .consent-panel {
    margin: 12px;
    padding: 12px;
    border: 1px solid color-mix(in srgb, var(--brass), transparent 35%);
    border-radius: 5px;
    background: color-mix(in srgb, var(--brass), transparent 94%);
  }
  .consent-panel header {
    display: flex;
    min-height: 0;
    justify-content: space-between;
    gap: 12px;
    padding: 0;
    border-bottom: 0;
    color: var(--brass);
    font-size: 0.75rem;
  }
  .consent-panel p {
    color: var(--muted-strong);
    font-size: 0.75rem;
    line-height: 1.6;
  }
  .consent-option {
    display: flex;
    align-items: start;
    gap: 8px;
    margin-top: 9px;
    color: var(--text-soft);
    font-size: 0.75rem;
    line-height: 1.5;
  }
  .consent-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 12px;
  }
  .operations-panel {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    border-top: 1px solid var(--border);
  }
  .operations-panel article {
    min-width: 0;
    padding: 14px;
    border-right: 1px solid var(--border);
  }
  .operations-panel article > header {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    color: var(--brass);
    font-size: 0.75rem;
  }
  .operations-panel article > p {
    min-height: 42px;
    color: var(--muted-strong);
    font-size: 0.75rem;
    line-height: 1.5;
  }
  .metric-row {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin-bottom: 10px;
    border: 1px solid var(--border);
  }
  .metric-row div {
    padding: 8px;
    border-bottom: 0;
  }
  .operation-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
  }
  .editor-actions,
  .inspector-actions {
    justify-content: flex-end;
    padding: 12px 14px;
    border-top: 1px solid var(--border);
  }
  .credential-note {
    padding: 12px 14px;
    border-top: 1px solid var(--border);
  }
  .empty,
  .state {
    padding: 24px;
  }
  .state {
    margin-top: 12px;
  }
  .state.error {
    border-color: color-mix(in srgb, var(--danger), transparent 35%);
  }
  .state button {
    margin-top: 12px;
  }
  @media (max-width: 960px) {
    .server-layout {
      grid-template-columns: 1fr;
    }
  }
  @media (max-width: 560px) {
    .page-head {
      align-items: stretch;
      flex-direction: column;
    }
    .head-actions button {
      flex: 1;
    }
    dl,
    .field-grid {
      grid-template-columns: 1fr;
    }
    .field-grid .wide {
      grid-column: auto;
    }
    .inspector-actions button,
    .editor-actions button {
      flex: 1;
    }
    .operations-panel {
      grid-template-columns: 1fr;
    }
  }
</style>
