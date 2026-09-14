import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import AsyncRouteState from './AsyncRouteState.svelte';

describe('AsyncRouteState', () => {
  it('announces loading without exposing a retry action', () => {
    render(AsyncRouteState, {
      state: 'loading',
      title: '정확한 게임 데이터를 확인하고 있습니다.',
      message: '검증된 데이터를 준비합니다.',
    });

    expect(screen.getByRole('region')).toHaveAttribute('aria-busy', 'true');
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('offers one clear recovery action for a safe error message', async () => {
    const onRetry = vi.fn<() => void>();
    render(AsyncRouteState, {
      state: 'error',
      title: '데이터를 불러오지 못했습니다.',
      message: '잠시 후 다시 시도해 주세요.',
      onRetry,
    });

    await fireEvent.click(screen.getByRole('button', { name: '다시 시도' }));
    expect(onRetry).toHaveBeenCalledOnce();
    expect(screen.queryByText('LOAD ERROR')).not.toBeInTheDocument();
  });
});
