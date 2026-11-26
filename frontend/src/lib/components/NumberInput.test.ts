import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { component, fakeBackend, sentChanges, setUp, tearDown } from './input-test-support';
import NumberInput from './NumberInput.svelte';

const spec = component({
  type: 'number_input',
  label: 'Bins',
  state: 'bins',
  min: 1,
  max: 200,
  step: null,
  integer: true,
  default: 20,
});

beforeEach(() => setUp({ bins: 20 }));
afterEach(tearDown);

describe('NumberInput', () => {
  it('shows the current value under its label, with the accepted range', () => {
    render(NumberInput, { props: { spec } });
    const input = screen.getByLabelText<HTMLInputElement>('Bins');
    expect(input.value).toBe('20');
    expect(input.min).toBe('1');
    expect(input.max).toBe('200');
    expect(input.getAttribute('aria-describedby')).not.toBeNull();
    expect(screen.getByText('A whole number from 1 to 200.')).toBeTruthy();
  });

  it('sends the number the user typed', async () => {
    render(NumberInput, { props: { spec } });
    const input = screen.getByLabelText<HTMLInputElement>('Bins');
    await fireEvent.change(input, { target: { value: '30' } });
    await waitFor(() => expect(sentChanges()).toEqual([['bins', 30]]));
    await waitFor(() => expect(input.value).toBe('30'));
  });

  it('asks for a whole number without sending anything', async () => {
    render(NumberInput, { props: { spec } });
    const input = screen.getByLabelText<HTMLInputElement>('Bins');
    await fireEvent.change(input, { target: { value: '2.5' } });
    expect(screen.getByRole('alert').textContent).toBe('Enter a whole number.');
    expect(sentChanges()).toEqual([]);
    expect(input.value).toBe('20');
  });

  it('announces why the application refused the value and shows the one it holds', async () => {
    fakeBackend({ refusal: 'invalid value for state `bins`: 300 is above the maximum 200' });
    render(NumberInput, { props: { spec } });
    const input = screen.getByLabelText<HTMLInputElement>('Bins');
    await fireEvent.change(input, { target: { value: '300' } });
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('300 is above the maximum 200');
    expect(input.value).toBe('20');
  });
});
