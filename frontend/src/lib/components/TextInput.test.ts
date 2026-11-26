import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { session } from '../session.svelte';
import { component, fakeBackend, sentChanges, setUp, tearDown } from './input-test-support';
import TextInput from './TextInput.svelte';

const spec = component({
  type: 'text_input',
  label: 'Title',
  state: 'title',
  default: 'Run',
  max_length: 8,
});

beforeEach(() => setUp({ title: 'Run' }));
afterEach(tearDown);

describe('TextInput', () => {
  it('shows the current text under its label, with its maximum length', () => {
    render(TextInput, { props: { spec } });
    expect(screen.getByLabelText<HTMLInputElement>('Title').value).toBe('Run');
    expect(screen.getByText('At most 8 characters.')).toBeTruthy();
  });

  it('sends the text the user typed', async () => {
    render(TextInput, { props: { spec } });
    await fireEvent.change(screen.getByLabelText('Title'), { target: { value: 'Run 2' } });
    await waitFor(() => expect(sentChanges()).toEqual([['title', 'Run 2']]));
  });

  it('counts characters, not UTF-16 units, as the application does', async () => {
    render(TextInput, { props: { spec } });
    const input = screen.getByLabelText<HTMLInputElement>('Title');
    // Eight characters, sixteen UTF-16 units.
    const eight = '😀'.repeat(8);
    await fireEvent.change(input, { target: { value: eight } });
    await waitFor(() => expect(sentChanges()).toEqual([['title', eight]]));
    await waitFor(() => expect(session.state['title']).toBe(eight));

    await fireEvent.change(input, { target: { value: '😀'.repeat(9) } });
    expect(screen.getByRole('alert').textContent).toBe(
      'Enter at most 8 characters; this text has 9.',
    );
    expect(sentChanges()).toHaveLength(1);
    expect(input.value).toBe(eight);
  });

  it('announces why the application refused the text and shows the one it holds', async () => {
    fakeBackend({ refusal: 'invalid value for state `title`: the text is not allowed' });
    render(TextInput, { props: { spec } });
    const input = screen.getByLabelText<HTMLInputElement>('Title');
    await fireEvent.change(input, { target: { value: 'Run 3' } });
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('the text is not allowed');
    expect(input.value).toBe('Run');
  });
});
