import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import PalCheckbox from './PalCheckbox.svelte';
import PalSlider from './PalSlider.svelte';
import PalSwitch from './PalSwitch.svelte';

describe('Bits UI control boundaries', () => {
  it('keeps switch state and its accessible name in the PalBeacon wrapper', async () => {
    render(PalSwitch, { checked: false, label: '오버레이 표시' });

    const control = screen.getByRole('switch', { name: '오버레이 표시' });
    expect(control).toHaveAttribute('aria-checked', 'false');
    await fireEvent.click(control);
    expect(control).toHaveAttribute('aria-checked', 'true');
  });

  it('keeps checkbox state and its accessible name in the PalBeacon wrapper', async () => {
    render(PalCheckbox, { checked: false, label: '보스 표시' });

    const control = screen.getByRole('checkbox', { name: '보스 표시' });
    expect(control).toHaveAttribute('aria-checked', 'false');
    await fireEvent.click(control);
    expect(control).toHaveAttribute('aria-checked', 'true');
  });

  it('supports keyboard adjustment through the Bits UI slider primitive', async () => {
    render(PalSlider, {
      value: 2,
      min: 1,
      max: 4,
      step: 0.25,
      label: '오버레이 확대',
    });

    const control = screen.getByRole('slider', { name: '오버레이 확대' });
    expect(control).toHaveAttribute('aria-valuenow', '2');
    control.focus();
    await fireEvent.keyDown(control, { key: 'ArrowRight' });
    expect(control).toHaveAttribute('aria-valuenow', '2.25');
  });
});
