import dayjs from 'dayjs';

const eur = new Intl.NumberFormat('de-DE', { style: 'currency', currency: 'EUR' });
const eur0 = new Intl.NumberFormat('de-DE', { style: 'currency', currency: 'EUR', maximumFractionDigits: 0 });
const numberFmt = (digits: number) =>
  new Intl.NumberFormat('de-DE', { minimumFractionDigits: digits, maximumFractionDigits: digits });

export const money = (v: number | null | undefined) => eur.format(v ?? 0);
export const moneyShort = (v: number | null | undefined) => eur0.format(v ?? 0);

/** Compact money for chart axes: 12,3 Tsd. € */
export const moneyCompact = (v: number) => {
  const abs = Math.abs(v);
  if (abs >= 1_000_000) return `${numberFmt(1).format(v / 1_000_000)} Mio. €`;
  if (abs >= 10_000) return `${numberFmt(0).format(v / 1000)} Tsd. €`;
  if (abs >= 1000) return `${numberFmt(1).format(v / 1000)} Tsd. €`;
  return `${numberFmt(0).format(v)} €`;
};

export const num = (v: number | null | undefined, digits = 2) => numberFmt(digits).format(v ?? 0);

export const qty = (v: number | null | undefined) =>
  new Intl.NumberFormat('de-DE', { maximumFractionDigits: 3 }).format(v ?? 0);

export const percent = (v: number | null | undefined) =>
  `${new Intl.NumberFormat('de-DE', { maximumFractionDigits: 2 }).format(v ?? 0)} %`;

export const date = (s: string | null | undefined) => (s ? dayjs(s).format('DD.MM.YYYY') : '—');
export const dateShort = (s: string | null | undefined) => (s ? dayjs(s).format('DD.MM.') : '—');
export const dateTime = (s: string | null | undefined) => (s ? dayjs(s).format('DD.MM.YYYY, HH:mm') : '—');

export const today = () => dayjs().format('YYYY-MM-DD');
export const isoDate = (d: dayjs.Dayjs) => d.format('YYYY-MM-DD');

export const relativeDays = (s: string | null | undefined) => {
  if (!s) return '';
  const diff = dayjs(s).startOf('day').diff(dayjs().startOf('day'), 'day');
  if (diff === 0) return 'heute';
  if (diff === 1) return 'morgen';
  if (diff === -1) return 'gestern';
  if (diff > 0) return `in ${diff} Tagen`;
  return `vor ${-diff} Tagen`;
};

export function formatIban(iban: string) {
  return iban.replace(/\s+/g, '').replace(/(.{4})/g, '$1 ').trim();
}

/** Commercial rounding to cents (half away from zero). */
export function r2(v: number) {
  const sign = v < 0 ? -1 : 1;
  return (sign * Math.round(Math.abs(v) * 100 + 1e-9)) / 100;
}
