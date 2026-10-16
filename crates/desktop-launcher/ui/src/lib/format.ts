const UNITS = ['B', 'KB', 'MB', 'GB'];

/** Formats a byte count in decimal units with one decimal place above a kilobyte. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  return `${unit === 0 ? value : value.toFixed(1)} ${UNITS[unit]}`;
}

const relative = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
const STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 86_400_000],
  ['month', 30 * 86_400_000],
  ['week', 7 * 86_400_000],
  ['day', 86_400_000],
  ['hour', 3_600_000],
  ['minute', 60_000],
];

/** Formats an ISO timestamp relative to `now`, e.g. "3 minutes ago"; under a minute reads "just now". */
export function formatAgo(iso: string, now: number): string {
  const delta = Date.parse(iso) - now;
  for (const [unit, ms] of STEPS) {
    if (Math.abs(delta) >= ms) return relative.format(Math.round(delta / ms), unit);
  }
  return 'just now';
}

const shortDate = new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric' });

export function formatDate(iso: string): string {
  return shortDate.format(new Date(iso));
}

const count = new Intl.NumberFormat('en');

export function formatCount(n: number): string {
  return count.format(n);
}

/** Experiences join by ID with port 0 and have no address worth showing. */
export function isExperience(port: number): boolean {
  return port === 0;
}

/** Bedrock's default port is implied, so only non-default ports are shown. */
export function formatAddress(address: string, port: number): string {
  if (isExperience(port)) return 'Experience';
  return port === 19132 ? address : `${address}:${port}`;
}
