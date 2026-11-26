import { describe, expect, it } from 'vitest';

import { formatBytes, formatCell, formatNumber } from './format';

describe('formatNumber', () => {
  it('groups the digits of integers', () => {
    expect(formatNumber(1234567)).toBe('1,234,567');
  });

  it('keeps six significant digits of decimals', () => {
    expect(formatNumber(292.2613556)).toBe('292.261');
    expect(formatNumber(0.022)).toBe('0.022');
  });

  it('uses exponent notation for very small and very large decimals', () => {
    expect(formatNumber(0.0000123456)).toBe('1.2346e-5');
    expect(formatNumber(1.5e12 + 0.5)).toBe('1.5000e+12');
  });

  it('shows a dash for missing and non-finite values', () => {
    expect(formatNumber(null)).toBe('—');
    expect(formatNumber(undefined)).toBe('—');
    expect(formatNumber(Number.NaN)).toBe('—');
  });
});

describe('formatCell', () => {
  it('formats numbers, keeps text and shows a dash for null', () => {
    expect(formatCell(1500)).toBe('1,500');
    expect(formatCell('Si-ref')).toBe('Si-ref');
    expect(formatCell(true)).toBe('true');
    expect(formatCell(null)).toBe('—');
  });
});

describe('formatBytes', () => {
  it('chooses a binary unit', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(7755)).toBe('7.6 KiB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MiB');
  });
});
