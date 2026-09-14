<script lang="ts">
  import { resolve } from '$app/paths';
  import RecentWorkspace from '$lib/shared/workspace/RecentWorkspace.svelte';
</script>

<section class="home pal-screen">
  <header class="welcome">
    <h1>통합 검색</h1>
    <form action={resolve('/search/', {})} method="get" role="search">
      <label for="home-search">통합 검색</label>
      <div>
        <input
          id="home-search"
          name="q"
          type="search"
          autocomplete="off"
          placeholder="팰, 아이템, 기술 이름"
        />
        <button type="submit">찾기</button>
      </div>
    </form>
  </header>

  <RecentWorkspace />
</section>

<style>
  .home {
    display: grid;
    gap: 14px;
    padding: clamp(16px, 3vw, 32px);
  }

  .welcome {
    display: grid;
    grid-template-columns: minmax(140px, auto) minmax(360px, 720px);
    gap: 24px;
    align-items: center;
    justify-content: space-between;
    padding: 18px 20px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--ink);
  }

  h1 {
    margin: 0;
    font-size: 1.2rem;
    letter-spacing: -0.035em;
  }

  form {
    display: grid;
    gap: 7px;
  }

  form label {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  form > div {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
  }

  input,
  button {
    min-height: 48px;
  }

  input {
    min-width: 0;
    padding: 0 14px;
    border: 1px solid var(--border-strong);
    border-right: 0;
    border-radius: 5px 0 0 5px;
    background: var(--void);
    font-size: 0.9rem;
  }

  input:focus {
    border-color: var(--accent);
    outline: 2px solid rgb(0 215 233 / 18%);
    outline-offset: 0;
  }

  button {
    padding-inline: 20px;
    border: 1px solid var(--accent);
    border-radius: 0 5px 5px 0;
    background: var(--accent-strong);
    color: var(--void);
    cursor: pointer;
    font-weight: 820;
  }

  @media (max-width: 980px) {
    .welcome {
      grid-template-columns: 1fr;
    }
  }

  @media (max-width: 719px) {
    .home {
      padding: 12px;
    }

    .welcome {
      gap: 18px;
      padding: 18px;
    }
  }
</style>
