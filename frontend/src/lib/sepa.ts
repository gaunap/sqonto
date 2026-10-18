// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// SEPA identifiers, checked like the backend (util.rs) to give early feedback in forms.

const normalize = (s: string) => s.replace(/\s+/g, '').toUpperCase();

function mod97(digits: string) {
  let rem = 0;
  for (const c of digits) {
    const d = parseInt(c, 36);
    rem = d < 10 ? (rem * 10 + d) % 97 : (rem * 100 + d) % 97;
  }
  return rem;
}

export function ibanValid(s: string) {
  const n = normalize(s);
  if (!/^[A-Z]{2}\d{2}[A-Z0-9]{11,30}$/.test(n)) return false;
  return mod97(n.slice(4) + n.slice(0, 4)) === 1;
}

/** Gläubiger-Identifikationsnummer: the business code (characters 5–7) is not part of the check. */
export function creditorIdValid(s: string) {
  const n = normalize(s);
  if (!/^[A-Z]{2}\d{2}[A-Z0-9]{4,31}$/.test(n)) return false;
  if (n.startsWith('DE') && !/^DE\d{2}[A-Z0-9]{3}\d{11}$/.test(n)) return false;
  return mod97(n.slice(7) + n.slice(0, 4)) === 1;
}

export const SCHEMES = [
  { value: 'CORE', label: 'Basislastschrift (CORE)' },
  { value: 'B2B', label: 'Firmenlastschrift (B2B)' },
];

export const SEQUENCES: Record<string, string> = { FRST: 'Erstlastschrift', RCUR: 'Folgelastschrift', OOFF: 'Einmallastschrift' };
