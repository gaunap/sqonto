// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// Client-side mirror of backend/src/services/calc.rs for instant totals in the editor.
import type { Document, DocumentLine, Receipt, TaxGroup, Totals } from '@/api/types';
import { r2 } from './format';

export function effectiveRate(doc: Pick<Document, 'tax_mode'>, line: Pick<DocumentLine, 'tax_rate'>) {
  return doc.tax_mode === 'standard' ? line.tax_rate : 0;
}

function distribute(total: number, weights: number[]): number[] {
  if (weights.length === 0) return [];
  const sum = weights.reduce((a, b) => a + b, 0);
  if (sum === 0) return weights.map((_, i) => (i === 0 ? total : 0));
  const parts = weights.map((w) => r2((total * w) / sum));
  const diff = r2(total - parts.reduce((a, b) => a + b, 0));
  if (diff !== 0) {
    let idx = 0;
    weights.forEach((w, i) => {
      if (Math.abs(w) > Math.abs(weights[idx])) idx = i;
    });
    parts[idx] = r2(parts[idx] + diff);
  }
  return parts;
}

export interface LineAmounts {
  net: number;
  tax: number;
  gross: number;
}

export function calculate(doc: Pick<Document, 'tax_mode' | 'prices_gross'>, lines: DocumentLine[]): { totals: Totals; amounts: LineAmounts[] } {
  const amounts: LineAmounts[] = lines.map(() => ({ net: 0, tax: 0, gross: 0 }));
  const groups = new Map<number, { sum: number; idx: number[] }>();
  lines.forEach((l, i) => {
    if (l.line_type === 'item') {
      const base = r2((l.quantity || 0) * (l.unit_price || 0) * (1 - (l.discount_percent || 0) / 100));
      const rate = effectiveRate(doc, l);
      if (doc.prices_gross) amounts[i].gross = base;
      else amounts[i].net = base;
      const g = groups.get(rate) ?? { sum: 0, idx: [] };
      g.sum += base;
      g.idx.push(i);
      groups.set(rate, g);
    } else if (l.line_type === 'deduction') {
      amounts[i] = { net: r2(l.net_amount), tax: r2(l.tax_amount), gross: r2(l.net_amount + l.tax_amount) };
    }
  });
  const itemGroups: TaxGroup[] = [];
  [...groups.entries()]
    .sort((a, b) => b[0] - a[0])
    .forEach(([rate, g]) => {
      const sum = r2(g.sum);
      let net: number, tax: number, gross: number;
      if (doc.prices_gross) {
        net = r2(sum / (1 + rate / 100));
        tax = r2(sum - net);
        gross = sum;
      } else {
        tax = r2((sum * rate) / 100);
        net = sum;
        gross = r2(sum + tax);
      }
      const weights = g.idx.map((i) => (doc.prices_gross ? amounts[i].gross : amounts[i].net));
      const taxes = distribute(tax, weights);
      g.idx.forEach((i, k) => {
        amounts[i].tax = taxes[k];
        if (doc.prices_gross) amounts[i].net = r2(amounts[i].gross - taxes[k]);
        else amounts[i].gross = r2(amounts[i].net + taxes[k]);
      });
      itemGroups.push({ rate, net, tax, gross });
    });
  // subtotals
  let runNet = 0;
  let runGross = 0;
  lines.forEach((l, i) => {
    if (l.line_type === 'item') {
      runNet += amounts[i].net;
      runGross += amounts[i].gross;
    } else if (l.line_type === 'subtotal') {
      amounts[i] = { net: r2(runNet), tax: 0, gross: r2(runGross) };
      runNet = 0;
      runGross = 0;
    }
  });
  const finalGroups = new Map<number, TaxGroup>(itemGroups.map((g) => [g.rate, { ...g }]));
  const deductions: Totals['deductions'] = [];
  lines.forEach((l, i) => {
    if (l.line_type !== 'deduction') return;
    const rate = effectiveRate(doc, l);
    const g = finalGroups.get(rate) ?? { rate, net: 0, tax: 0, gross: 0 };
    g.net = r2(g.net + amounts[i].net);
    g.tax = r2(g.tax + amounts[i].tax);
    g.gross = r2(g.gross + amounts[i].gross);
    finalGroups.set(rate, g);
    const existing = deductions.find((d) => d.document_id !== null && d.document_id === l.ref_document_id);
    if (existing) {
      existing.net = r2(existing.net + amounts[i].net);
      existing.tax = r2(existing.tax + amounts[i].tax);
      existing.gross = r2(existing.gross + amounts[i].gross);
    } else {
      deductions.push({ document_id: l.ref_document_id, label: l.name, ...amounts[i] });
    }
  });
  const groupsArr = [...finalGroups.values()].sort((a, b) => b.rate - a.rate);
  const itemsNet = r2(itemGroups.reduce((a, g) => a + g.net, 0));
  const itemsTax = r2(itemGroups.reduce((a, g) => a + g.tax, 0));
  const net = r2(groupsArr.reduce((a, g) => a + g.net, 0));
  const tax = r2(groupsArr.reduce((a, g) => a + g.tax, 0));
  return {
    amounts,
    totals: {
      items_net: itemsNet,
      items_tax: itemsTax,
      items_gross: r2(itemsNet + itemsTax),
      item_groups: itemGroups,
      deductions,
      groups: groupsArr,
      net,
      tax,
      gross: r2(net + tax),
    },
  };
}

// Mirrors Receipt::money_sign / remaining: supplier credit notes (negative receipts) are settled
// by money received, like income receipts.
export function receiptIncoming(r: Pick<Receipt, 'receipt_type' | 'gross_total'>) {
  return (r.receipt_type === 'income') !== (r.gross_total < 0);
}

/** Amount still to be paid or refunded – positive while the receipt is open. */
export function receiptRemaining(r: Pick<Receipt, 'gross_total'>, openAmount: number) {
  return openAmount * Math.sign(r.gross_total);
}
