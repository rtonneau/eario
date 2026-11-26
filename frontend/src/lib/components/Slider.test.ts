import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { component, fakeBackend, sentChanges, setUp, tearDown } from './input-test-support';
import Slider from './Slider.svelte';

const spec = component({
  type: 'slider',
  label: 'Window',
  state: 'window',
  min: 0,
  max: 1,
  step: 0.1,
  integer: false,
  default: 0.3,
});

beforeEach(() => setUp({ window: 0.3 }));
afterEach(tearDown);

describe('Slider', () => {
  it('shows the current value next to the labelled slider', () => {
    render(Slider, { props: { spec } });
    const slider = screen.getByLabelText<HTMLInputElement>('Window');
    expect(slider.type).toBe('range');
    expect(slider.value).toBe('0.3');
    expect(screen.getByRole('status').textContent).toBe('0.3');
  });

  it('follows the drag and sends the value when it is released', async () => {
    render(Slider, { props: { spec } });
    const slider = screen.getByLabelText<HTMLInputElement>('Window');
    await fireEvent.input(slider, { target: { value: '0.5' } });
    expect(screen.getByRole('status').textContent).toBe('0.5');
    expect(sentChanges()).toEqual([]);

    await fireEvent.change(slider);
    await waitFor(() => expect(sentChanges()).toEqual([['window', 0.5]]));
  });

  it('announces a refusal and moves back to the value the application holds', async () => {
    fakeBackend({ refusal: 'invalid value for state `window`: 0.9 is above the maximum 0.8' });
    render(Slider, { props: { spec } });
    const slider = screen.getByLabelText<HTMLInputElement>('Window');
    await fireEvent.change(slider, { target: { value: '0.9' } });
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('0.9 is above the maximum 0.8');
    await waitFor(() => expect(slider.value).toBe('0.3'));
    expect(screen.getByRole('status').textContent).toBe('0.3');
  });
});
