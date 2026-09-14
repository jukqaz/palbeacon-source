<script lang="ts">
  import { resolve } from '$app/paths';

  interface Props {
    active: 'active' | 'passive';
  }

  let { active }: Props = $props();

  const skillPath = (kind: 'active' | 'passive'): string =>
    resolve('/[...path]', { path: `skills/${kind}` });
</script>

<nav class="skill-type-navigation" aria-label="스킬 종류">
  <a
    href={skillPath('active')}
    class:active={active === 'active'}
    aria-current={active === 'active' ? 'page' : undefined}>액티브</a
  >
  <a
    href={skillPath('passive')}
    class:active={active === 'passive'}
    aria-current={active === 'passive' ? 'page' : undefined}>패시브</a
  >
</nav>

<style>
  .skill-type-navigation {
    display: flex;
    width: fit-content;
    margin: 16px 20px 0;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    overflow: hidden;
    background: var(--ink);
  }

  a {
    display: inline-flex;
    min-width: 88px;
    min-height: 40px;
    align-items: center;
    justify-content: center;
    padding-inline: 14px;
    color: var(--muted-strong);
    text-decoration: none;
    font-size: 0.78rem;
    font-weight: 760;
  }

  a + a {
    border-left: 1px solid var(--border-strong);
  }

  a:hover,
  a.active {
    color: var(--text);
    background: var(--surface-raised);
    box-shadow: inset 0 -3px 0 var(--accent);
  }

  @media (max-width: 719px) {
    .skill-type-navigation {
      width: calc(100% - 24px);
      margin: 12px 12px 0;
    }

    a {
      flex: 1;
    }
  }
</style>
