//! Document totals: line amounts, VAT groups, deductions of partial invoices.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::models::{Document, DocumentLine};
use crate::util::{HUNDRED, r2};

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaxGroup {
    #[ts(type = "number")]
    pub rate: Decimal,
    #[ts(type = "number")]
    pub net: Decimal,
    #[ts(type = "number")]
    pub tax: Decimal,
    #[ts(type = "number")]
    pub gross: Decimal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeductionInfo {
    pub document_id: Option<i64>,
    pub label: String,
    #[ts(type = "number")]
    pub net: Decimal,
    #[ts(type = "number")]
    pub tax: Decimal,
    #[ts(type = "number")]
    pub gross: Decimal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Totals {
    #[ts(type = "number")]
    pub items_net: Decimal,
    #[ts(type = "number")]
    pub items_tax: Decimal,
    #[ts(type = "number")]
    pub items_gross: Decimal,
    /// VAT groups of the positions (before deductions).
    pub item_groups: Vec<TaxGroup>,
    pub deductions: Vec<DeductionInfo>,
    /// VAT groups after deductions (what is actually invoiced / booked).
    pub groups: Vec<TaxGroup>,
    #[ts(type = "number")]
    pub net: Decimal,
    #[ts(type = "number")]
    pub tax: Decimal,
    #[ts(type = "number")]
    pub gross: Decimal,
}

pub fn effective_rate(doc: &Document, line: &DocumentLine) -> Decimal {
    if doc.tax_mode == "standard" { line.tax_rate } else { Decimal::ZERO }
}

fn line_base(line: &DocumentLine) -> Decimal {
    let disc = Decimal::ONE - line.discount_percent / HUNDRED;
    r2(line.quantity * line.unit_price * disc)
}

/// Distributes `total` over `weights` so that the parts sum up exactly (largest remainder).
pub fn distribute(total: Decimal, weights: &[Decimal]) -> Vec<Decimal> {
    let sum: Decimal = weights.iter().copied().sum();
    if weights.is_empty() {
        return vec![];
    }
    if sum.is_zero() {
        let mut v = vec![Decimal::ZERO; weights.len()];
        v[0] = total;
        return v;
    }
    let mut parts: Vec<Decimal> = weights.iter().map(|w| r2(total * *w / sum)).collect();
    let diff = total - parts.iter().copied().sum::<Decimal>();
    if !diff.is_zero() {
        // Assign the rounding difference to the largest absolute weight.
        let idx = weights
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().cmp(&b.1.abs()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        parts[idx] += diff;
    }
    parts
}

/// Calculates all amounts. Updates `net_amount`, `tax_amount` and `gross_amount`
/// on every line (except stored deduction amounts) and returns the totals.
pub fn calculate(doc: &Document, lines: &mut [DocumentLine]) -> Totals {
    // 1. Item bases per line.
    let mut groups: BTreeMap<Decimal, (Decimal, Vec<usize>)> = BTreeMap::new();
    for (i, line) in lines.iter_mut().enumerate() {
        match line.line_type.as_str() {
            "item" => {
                let base = line_base(line);
                let rate = effective_rate(doc, line);
                if doc.prices_gross {
                    line.gross_amount = base;
                } else {
                    line.net_amount = base;
                }
                let e = groups.entry(rate.normalize()).or_insert((Decimal::ZERO, vec![]));
                e.0 += base;
                e.1.push(i);
            }
            "deduction" => {
                line.net_amount = r2(line.net_amount);
                line.tax_amount = r2(line.tax_amount);
                line.gross_amount = line.net_amount + line.tax_amount;
            }
            _ => {
                line.net_amount = Decimal::ZERO;
                line.tax_amount = Decimal::ZERO;
                line.gross_amount = Decimal::ZERO;
            }
        }
    }

    // 2. VAT per group, distribute back onto lines.
    let mut item_groups = Vec::new();
    for (rate, (sum, idxs)) in groups.iter().rev() {
        let (net, tax, gross) = if doc.prices_gross {
            let net = r2(*sum / (Decimal::ONE + *rate / HUNDRED));
            (net, *sum - net, *sum)
        } else {
            let tax = r2(*sum * *rate / HUNDRED);
            (*sum, tax, *sum + tax)
        };
        let weights: Vec<Decimal> =
            idxs.iter().map(|i| if doc.prices_gross { lines[*i].gross_amount } else { lines[*i].net_amount }).collect();
        let taxes = distribute(tax, &weights);
        for (k, i) in idxs.iter().enumerate() {
            let l = &mut lines[*i];
            l.tax_amount = taxes[k];
            if doc.prices_gross {
                l.net_amount = l.gross_amount - l.tax_amount;
            } else {
                l.gross_amount = l.net_amount + l.tax_amount;
            }
        }
        item_groups.push(TaxGroup { rate: *rate, net, tax, gross });
    }

    // 3. Subtotals (running since the previous subtotal).
    let mut run_net = Decimal::ZERO;
    let mut run_gross = Decimal::ZERO;
    for line in lines.iter_mut() {
        match line.line_type.as_str() {
            "item" => {
                run_net += line.net_amount;
                run_gross += line.gross_amount;
            }
            "subtotal" => {
                line.net_amount = run_net;
                line.gross_amount = run_gross;
                run_net = Decimal::ZERO;
                run_gross = Decimal::ZERO;
            }
            _ => {}
        }
    }

    // 4. Deductions (final invoices).
    let mut deductions: Vec<DeductionInfo> = Vec::new();
    let mut final_groups: BTreeMap<Decimal, TaxGroup> =
        item_groups.iter().map(|g| (g.rate, g.clone())).collect();
    for line in lines.iter().filter(|l| l.line_type == "deduction") {
        let rate = effective_rate(doc, line).normalize();
        let g = final_groups.entry(rate).or_insert(TaxGroup { rate, ..Default::default() });
        g.net += line.net_amount;
        g.tax += line.tax_amount;
        g.gross += line.gross_amount;
        match deductions.iter_mut().find(|d| d.document_id.is_some() && d.document_id == line.ref_document_id) {
            Some(d) => {
                d.net += line.net_amount;
                d.tax += line.tax_amount;
                d.gross += line.gross_amount;
            }
            None => deductions.push(DeductionInfo {
                document_id: line.ref_document_id,
                label: line.name.clone(),
                net: line.net_amount,
                tax: line.tax_amount,
                gross: line.gross_amount,
            }),
        }
    }
    let groups: Vec<TaxGroup> = final_groups.into_values().rev().collect();

    let items_net = item_groups.iter().map(|g| g.net).sum();
    let items_tax = item_groups.iter().map(|g| g.tax).sum();
    let net: Decimal = groups.iter().map(|g| g.net).sum();
    let tax: Decimal = groups.iter().map(|g| g.tax).sum();
    Totals {
        items_net,
        items_tax,
        items_gross: items_net + items_tax,
        item_groups,
        deductions,
        groups,
        net,
        tax,
        gross: net + tax,
    }
}

/// Applies totals onto the document header.
pub fn apply_totals(doc: &mut Document, t: &Totals) {
    doc.net_total = t.net;
    doc.tax_total = t.tax;
    doc.gross_total = t.gross;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn item(qty: Decimal, price: Decimal, rate: Decimal) -> DocumentLine {
        DocumentLine {
            line_type: "item".into(),
            quantity: qty,
            unit_price: price,
            tax_rate: rate,
            ..Default::default()
        }
    }

    fn doc(gross: bool) -> Document {
        Document { tax_mode: "standard".into(), prices_gross: gross, ..Default::default() }
    }

    #[test]
    fn net_prices_mixed_rates() {
        let d = doc(false);
        let mut lines = vec![item(dec!(3), dec!(33.33), dec!(19)), item(dec!(1), dec!(10), dec!(7))];
        let t = calculate(&d, &mut lines);
        assert_eq!(t.net, dec!(109.99));
        assert_eq!(t.groups[0].rate, dec!(19));
        assert_eq!(t.groups[0].tax, dec!(19.00));
        assert_eq!(t.groups[1].tax, dec!(0.70));
        assert_eq!(t.gross, dec!(129.69));
        assert_eq!(lines[0].gross_amount, dec!(118.99));
    }

    #[test]
    fn gross_prices() {
        let d = doc(true);
        let mut lines = vec![item(dec!(1), dec!(119), dec!(19)), item(dec!(2), dec!(9.99), dec!(19))];
        let t = calculate(&d, &mut lines);
        assert_eq!(t.gross, dec!(138.98));
        assert_eq!(t.net + t.tax, t.gross);
        assert_eq!(lines.iter().map(|l| l.net_amount).sum::<Decimal>(), t.net);
    }

    #[test]
    fn discount_and_small_business() {
        let mut d = doc(false);
        d.tax_mode = "small_business".into();
        let mut l = item(dec!(2), dec!(50), dec!(19));
        l.discount_percent = dec!(10);
        let mut lines = vec![l];
        let t = calculate(&d, &mut lines);
        assert_eq!(t.net, dec!(90));
        assert_eq!(t.tax, dec!(0));
        assert_eq!(t.gross, dec!(90));
    }

    #[test]
    fn final_invoice_deductions() {
        let d = doc(false);
        let mut ded = DocumentLine {
            line_type: "deduction".into(),
            name: "Abschlagsrechnung RE-1".into(),
            ref_document_id: Some(5),
            tax_rate: dec!(19),
            ..Default::default()
        };
        ded.net_amount = dec!(-300);
        ded.tax_amount = dec!(-57);
        let mut lines = vec![item(dec!(10), dec!(100), dec!(19)), ded];
        let t = calculate(&d, &mut lines);
        assert_eq!(t.items_gross, dec!(1190));
        assert_eq!(t.net, dec!(700));
        assert_eq!(t.tax, dec!(133));
        assert_eq!(t.gross, dec!(833));
        assert_eq!(t.deductions.len(), 1);
    }

    #[test]
    fn distribute_sums_exactly() {
        let parts = distribute(dec!(10), &[dec!(1), dec!(1), dec!(1)]);
        assert_eq!(parts.iter().copied().sum::<Decimal>(), dec!(10));
    }
}
