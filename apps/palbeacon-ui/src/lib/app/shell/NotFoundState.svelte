<script lang="ts">
  import { resolvePalPath } from '$lib/app/navigation/routes';

  interface Props {
    path: string;
    showPath?: boolean;
  }

  let { path, showPath = true }: Props = $props();
</script>

<section class="not-found" role="status">
  <h1>페이지를 찾을 수 없습니다.</h1>
  {#if showPath && path.length > 0}
    <p>요청한 페이지가 없거나 사용할 수 없습니다.</p>
  {:else}
    <p>요청한 경로는 PalBeacon Web 메뉴에 등록되어 있지 않습니다.</p>
  {/if}
  <div>
    <a class="primary" href={resolvePalPath('/')}>홈으로 돌아가기</a>
    <a href={resolvePalPath('/search/')}>통합 검색 열기</a>
  </div>
</section>

<style>
  .not-found {
    display: grid;
    min-height: min(52vh, 420px);
    place-items: center;
    align-content: center;
    padding: 32px;
    text-align: center;
  }

  h1 {
    margin: 0 0 8px;
    font-size: 1.35rem;
  }

  p {
    max-width: 560px;
    margin: 0;
    color: var(--muted-strong);
    font-size: 0.86rem;
    line-height: 1.7;
  }

  div {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
  }

  a {
    display: inline-flex;
    min-height: 44px;
    align-items: center;
    margin-top: 20px;
    padding-inline: 15px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--surface-raised);
    text-decoration: none;
    font-size: 0.78rem;
    font-weight: 750;
  }

  a.primary {
    border-color: color-mix(in srgb, var(--accent), transparent 25%);
    background: color-mix(in srgb, var(--accent), var(--surface-raised) 82%);
  }

  @media (max-width: 719px) {
    .not-found {
      min-height: 300px;
      padding: 20px;
    }

    h1 {
      font-size: 1.25rem;
    }
  }
</style>
