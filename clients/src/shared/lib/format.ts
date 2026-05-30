// Tiny presentation helpers shared by every persona. Ported from the mockup script blocks.

const AVATAR_PALETTE = ['#2563eb', '#0e6e8e', '#7c3aed', '#15803d', '#b45309', '#be185d'];

export function avatarColor(name: string): string {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return AVATAR_PALETTE[h % AVATAR_PALETTE.length];
}

export function initials(name: string): string {
  return name
    .replace(/[^A-Za-z ]/g, '')
    .split(' ')
    .filter(Boolean)
    .slice(0, 2)
    .map((s) => s[0])
    .join('')
    .toUpperCase();
}

/** Match the mockup's tri-state confidence colour: green >=85, amber >=60, red below. */
export function confColor(p: number): string {
  return p >= 85 ? 'var(--good)' : p >= 60 ? 'var(--warn)' : 'var(--bad)';
}

export function truncate(s: unknown, n: number): string {
  const str = String(s ?? '');
  return str.length > n ? `${str.slice(0, n - 1)}…` : str;
}

export function classNames(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(' ');
}
