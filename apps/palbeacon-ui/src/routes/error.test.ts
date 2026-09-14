import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ErrorPage from './+error.svelte';

const context = vi.hoisted(() => ({
  status: 404,
  url: new URL('https://palbeacon.jukqaz.xyz/missing-internal-route'),
  error: { message: 'private error token=secret C:\\private\\save.sav' },
}));

vi.mock('$app/state', () => ({ page: context }));

describe('official route error boundary', () => {
  it('uses the Korean recovery screen without echoing the route or error', () => {
    context.status = 404;
    const { container } = render(ErrorPage);
    expect(screen.getByRole('heading', { name: '페이지를 찾을 수 없습니다.' })).toBeVisible();
    expect(screen.getByRole('link', { name: '홈으로 돌아가기' })).toBeVisible();
    expect(container.textContent).not.toMatch(
      /404|Not Found|missing-internal-route|private|secret/,
    );
  });

  it('offers a Korean retry action for server failures without diagnostic labels', () => {
    context.status = 500;
    const { container } = render(ErrorPage);
    expect(screen.getByRole('heading', { name: '화면을 불러올 수 없습니다.' })).toBeVisible();
    expect(screen.getByRole('button', { name: '다시 시도' })).toBeVisible();
    expect(container.textContent).not.toMatch(/500|LOAD ERROR|private|secret|save\.sav/);
  });
});
