// Formatting of values for display.

const MISSING = '—';

const integerFormat = new Intl.NumberFormat('en-US');

/** Formats a number with at most six significant digits. */
export function formatNumber(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return MISSING;
  }
  if (Number.isInteger(value) && Math.abs(value) < 1e15) {
    return integerFormat.format(value);
  }
  const magnitude = Math.abs(value);
  if (magnitude !== 0 && (magnitude >= 1e9 || magnitude < 1e-4)) {
    return value.toExponential(4);
  }
  return String(Number(value.toPrecision(6)));
}

/** Formats a table cell: numbers as numbers, missing values as a dash. */
export function formatCell(value: unknown): string {
  if (value === null || value === undefined) {
    return MISSING;
  }
  if (typeof value === 'number') {
    return formatNumber(value);
  }
  return String(value);
}

/** Formats a size in bytes with a binary unit. */
export function formatBytes(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB'];
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${size} B` : `${size.toFixed(1)} ${units[unit]}`;
}
