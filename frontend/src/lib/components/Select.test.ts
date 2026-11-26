import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { component, fakeBackend, sentChanges, setUp, tearDown } from './input-test-support';
import Select from './Select.svelte';

const spec = component({
  type: 'select',
  label: 'Sample',
  state: 'sample',
  options: { type: 'column', value: { dataset: 'data', column: 'sample' } },
  default: null,
});

beforeEach(() => {
  setUp({ sample: 'Mylar' });
  fakeBackend({ data: { type: 'select', options: ['Au-thin', 'Mylar'], truncated: false } });
});
afterEach(tearDown);

describe('Select', () => {
  it('offers the values from the application and shows the current one', async () => {
    render(Select, { props: { spec } });
    await screen.findByRole('option', { name: 'Au-thin' });
    const select = screen.getByLabelText<HTMLSelectElement>('Sample');
    expect(select.value).toBe('Mylar');
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('sends the value the user chose', async () => {
    render(Select, { props: { spec } });
    await screen.findByRole('option', { name: 'Au-thin' });
    await fireEvent.change(screen.getByLabelText('Sample'), { target: { value: 'Au-thin' } });
    await waitFor(() => expect(sentChanges()).toEqual([['sample', 'Au-thin']]));
  });

  it('says when only the first values are offered', async () => {
    fakeBackend({ data: { type: 'select', options: ['a', 'b'], truncated: true } });
    render(Select, { props: { spec } });
    const status = await screen.findByRole('status');
    expect(status.textContent?.trim()).toBe('Only the first 2 values are offered.');
  });

  it('explains why nothing is on offer yet', async () => {
    fakeBackend({ data: { type: 'empty', reason: 'No dataset loaded yet.' } });
    render(Select, { props: { spec } });
    expect(await screen.findByText('No dataset loaded yet.')).toBeTruthy();
    expect(screen.getByLabelText<HTMLSelectElement>('Sample').disabled).toBe(true);
  });

  it('announces why the application refused the value and shows the one it holds', async () => {
    render(Select, { props: { spec } });
    await screen.findByRole('option', { name: 'Au-thin' });
    fakeBackend({ refusal: 'invalid value for state `sample`: `Au-thin` is not on offer' });
    const select = screen.getByLabelText<HTMLSelectElement>('Sample');
    await fireEvent.change(select, { target: { value: 'Au-thin' } });
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('is not on offer');
    expect(select.value).toBe('Mylar');
  });
});
