import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import Checkbox from './Checkbox.svelte';
import { component, fakeBackend, sentChanges, setUp, tearDown } from './input-test-support';

const spec = component({ type: 'checkbox', label: 'Show outliers', state: 'outliers', default: false });

beforeEach(() => setUp({ outliers: false }));
afterEach(tearDown);

describe('Checkbox', () => {
  it('shows the current value with its label', () => {
    render(Checkbox, { props: { spec } });
    expect(screen.getByLabelText<HTMLInputElement>('Show outliers').checked).toBe(false);
  });

  it('sends the new value when the user ticks it', async () => {
    render(Checkbox, { props: { spec } });
    const checkbox = screen.getByLabelText<HTMLInputElement>('Show outliers');
    await fireEvent.click(checkbox);
    await waitFor(() => expect(sentChanges()).toEqual([['outliers', true]]));
    expect(checkbox.checked).toBe(true);
  });

  it('announces a refusal and shows the value the application holds', async () => {
    fakeBackend({ refusal: 'invalid value for state `outliers`: expected true or false' });
    render(Checkbox, { props: { spec } });
    const checkbox = screen.getByLabelText<HTMLInputElement>('Show outliers');
    await fireEvent.click(checkbox);
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('expected true or false');
    expect(checkbox.checked).toBe(false);
  });
});
