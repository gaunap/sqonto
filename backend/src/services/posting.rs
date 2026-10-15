//! Posting engine: derives double-entry journal entries (Buchungssätze) from
//! business events. The journal is regenerated per source whenever the source
//! changes, so it always reflects the current state of documents, receipts,
//! payments, assets and manual entries.
//!
//! * EÜR (Einnahmen-Überschuss-Rechnung): revenue and expenses are booked on
//!   payment (Zufluss-/Abflussprinzip, Ist-Versteuerung).
//! * Bilanzierung (doppelte Buchführung): invoices and receipts are booked on
//!   their document date against Forderungen / Verbindlichkeiten (Soll-Versteuerung);
//!   payments clear these accounts.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;

use crate::chart;
use crate::db::{self, v};
use crate::models::*;
use crate::services::calc::{self, distribute};
use crate::services::{assets, settings as cfg};
use crate::util::{HUNDRED, r2, today, ymd};

#[derive(Debug, Clone, Copy)]
pub enum Source {
    Document(i64),
    Receipt(i64),
    Transaction(i64),
    Writeoff(i64),
    Asset(i64),
    Manual(i64),
    Opening(i64),
}

impl Source {
    fn key(&self) -> (&'static str, i64) {
        match *self {
            Source::Document(i) => ("document", i),
            Source::Receipt(i) => ("receipt", i),
            Source::Transaction(i) => ("transaction", i),
            Source::Writeoff(i) => ("writeoff", i),
            Source::Asset(i) => ("asset", i),
            Source::Manual(i) => ("manual", i),
            Source::Opening(i) => ("opening", i),
        }
    }
}

/// One posting line: positive amount = Soll (debit), negative = Haben (credit).
#[derive(Debug, Clone)]
struct PL {
    account: String,
    amount: Decimal,
    code: &'static str,
    base: Decimal,
}

fn pl(account: impl Into<String>, amount: Decimal) -> PL {
    PL { account: account.into(), amount, code: "", base: Decimal::ZERO }
}

fn plc(account: impl Into<String>, amount: Decimal, code: &'static str) -> PL {
    PL { account: account.into(), amount, code, base: Decimal::ZERO }
}

fn plt(account: impl Into<String>, amount: Decimal, code: &'static str, base: Decimal) -> PL {
    PL { account: account.into(), amount, code, base }
}

struct EntryDraft {
    date: NaiveDate,
    description: String,
    doc_ref: String,
    contact_id: Option<i64>,
    lines: Vec<PL>,
}

struct Ctx {
    chart: String,
    euer: bool,
    small_business: bool,
    accounts: HashMap<String, Account>,
}

impl Ctx {
    fn load(conn: &Connection) -> Result<Self> {
        let s = cfg::load(conn)?;
        let accounts = cfg::accounts(conn)?.into_iter().map(|a| (a.number.clone(), a)).collect();
        Ok(Self {
            chart: s.tax.chart.clone(),
            euer: s.tax.bookkeeping != "bilanz",
            small_business: s.tax.small_business,
            accounts,
        })
    }
    fn role(&self, r: &str) -> String {
        chart::role(&self.chart, r)
    }
    fn ust_account(&self, rate: Decimal) -> (String, &'static str) {
        if rate == Decimal::from(19) {
            (self.role("ust19"), "UST19")
        } else if rate == Decimal::from(7) {
            (self.role("ust7"), "UST7")
        } else {
            (self.role("ust"), "USTX")
        }
    }
    fn vst_account(&self, rate: Decimal) -> String {
        if rate == Decimal::from(19) {
            self.role("vst19")
        } else if rate == Decimal::from(7) {
            self.role("vst7")
        } else {
            self.role("vst")
        }
    }
    fn account_code(&self, number: &str) -> &'static str {
        match self.accounts.get(number).map(|a| a.tax_code.as_str()) {
            Some("FREI") => "FREI",
            Some("KU") => "KU",
            Some("IGL") => "IGL",
            Some("EXP") => "EXP",
            Some("EURC") => "EURC",
            Some("RC13B_OUT") => "RC13B_OUT",
            Some("NSTB") => "NSTB",
            _ => "",
        }
    }
    fn is_revenue(&self, number: &str) -> bool {
        self.accounts.get(number).map(|a| a.kind == "revenue").unwrap_or(false)
    }
}

fn base_code_for_rate(rate: Decimal) -> &'static str {
    if rate == Decimal::from(19) {
        "S19"
    } else if rate == Decimal::from(7) {
        "S7"
    } else if rate.is_zero() {
        "FREI"
    } else {
        "SX"
    }
}

/// Revenue split of a sales document: (revenue account, base code, net, tax account, tax code, tax, rate)
#[derive(Debug, Clone)]
struct Part {
    account: String,
    code: &'static str,
    net: Decimal,
    tax_account: String,
    tax_code: &'static str,
    tax: Decimal,
}

fn doc_parts(ctx: &Ctx, conn: &Connection, doc: &Document) -> Result<(Vec<Part>, Decimal)> {
    let mut lines = db::query::<DocumentLine>(conn, "WHERE document_id = ?1 ORDER BY position", &[v(doc.id)])?;
    let totals = calc::calculate(doc, &mut lines);
    let mode = doc.tax_mode.as_str();
    let mode_code: Option<&'static str> = match mode {
        "small_business" => Some("KU"),
        "reverse_charge" => Some("RC13B_OUT"),
        "eu_service" => Some("EURC"),
        "eu_delivery" => Some("IGL"),
        "export" => Some("EXP"),
        "third_country" => Some("NSTB"),
        _ => None,
    };
    let default_account = |rate: Decimal| -> String {
        let r = match mode {
            "small_business" => "erl_ku",
            "reverse_charge" => "erl_13b",
            "eu_service" => "erl_eu_rc",
            "eu_delivery" => "erl_igl",
            "export" => "erl_export",
            "third_country" => "erl_nstb",
            _ => {
                if rate == Decimal::from(19) {
                    "erl19"
                } else if rate == Decimal::from(7) {
                    "erl7"
                } else if rate.is_zero() {
                    "erl_frei"
                } else {
                    "erl"
                }
            }
        };
        ctx.role(r)
    };
    // Article revenue accounts.
    let mut article_acc: HashMap<i64, String> = HashMap::new();
    for l in &lines {
        if let Some(aid) = l.article_id
            && let Some(a) = db::find::<Article>(conn, aid)?
            && !a.revenue_account.is_empty()
        {
            article_acc.insert(aid, a.revenue_account);
        }
    }
    // Net per (rate, account)
    let mut by_rate: HashMap<Decimal, Vec<(String, Decimal)>> = HashMap::new();
    for l in lines.iter().filter(|l| l.line_type == "item" || l.line_type == "deduction") {
        let rate = calc::effective_rate(doc, l).normalize();
        let acc = if !l.account.is_empty() && ctx.accounts.contains_key(&l.account) {
            l.account.clone()
        } else if let Some(a) = l.article_id.and_then(|id| article_acc.get(&id)) {
            // Only use article accounts in standard taxation; special tax modes need their accounts.
            if mode == "standard" { a.clone() } else { default_account(rate) }
        } else {
            default_account(rate)
        };
        let e = by_rate.entry(rate).or_default();
        match e.iter_mut().find(|(a, _)| *a == acc) {
            Some(x) => x.1 += l.net_amount,
            None => e.push((acc, l.net_amount)),
        }
    }
    let mut parts = Vec::new();
    for g in &totals.groups {
        let rate = g.rate.normalize();
        let accs = by_rate.remove(&rate).unwrap_or_else(|| vec![(default_account(rate), g.net)]);
        let weights: Vec<Decimal> = accs.iter().map(|(_, n)| *n).collect();
        let taxes = distribute(g.tax, &weights);
        let nets = distribute(g.net, &weights);
        let (tax_account, tax_code) = ctx.ust_account(rate);
        for (i, (acc, _)) in accs.into_iter().enumerate() {
            let code = mode_code.unwrap_or_else(|| base_code_for_rate(rate));
            parts.push(Part { account: acc, code, net: nets[i], tax_account: tax_account.clone(), tax_code, tax: taxes[i] });
        }
    }
    Ok((parts, totals.gross))
}

/// Receipt split: (account, net, tax, rate)
fn receipt_parts(conn: &Connection, r: &Receipt) -> Result<Vec<ReceiptLine>> {
    db::query::<ReceiptLine>(conn, "WHERE receipt_id = ?1 ORDER BY position", &[v(r.id)])
}

/// Splits a (signed) payment amount proportionally over the parts of a document.
/// Returns posting lines that, together with the money account line (+amount), balance.
fn split_payment_doc(parts: &[Part], gross: Decimal, amount: Decimal) -> Vec<PL> {
    if gross.is_zero() {
        return vec![];
    }
    let mut weights = Vec::new();
    for p in parts {
        weights.push(p.net);
        weights.push(p.tax);
    }
    // Scale the full split: total of parts = amount.
    let shares = distribute(amount, &weights);
    let mut out = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let net = shares[i * 2];
        let tax = shares[i * 2 + 1];
        if !net.is_zero() {
            out.push(plc(p.account.clone(), -net, p.code));
        }
        if !tax.is_zero() {
            out.push(plt(p.tax_account.clone(), -tax, p.tax_code, -net));
        }
    }
    out
}

fn receipt_cost_lines(ctx: &Ctx, r: &Receipt, lines: &[ReceiptLine], factor_amount: Decimal) -> Vec<PL> {
    // factor_amount: signed amount to distribute; for expenses negative (money out) => debits positive.
    // We distribute -factor_amount over net+tax weights.
    let reverse_charge = r.tax_mode != "standard";
    let mut weights = Vec::new();
    for l in lines {
        if ctx.small_business || reverse_charge {
            weights.push(if reverse_charge { l.net } else { l.gross });
            weights.push(Decimal::ZERO);
        } else {
            weights.push(l.net);
            weights.push(l.tax);
        }
    }
    let shares = distribute(-factor_amount, &weights);
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let net = shares[i * 2];
        let tax = shares[i * 2 + 1];
        if !net.is_zero() {
            let code = if r.receipt_type == "income" && ctx.is_revenue(&l.account) {
                let c = ctx.account_code(&l.account);
                if c.is_empty() { base_code_for_rate(l.tax_rate) } else { c }
            } else {
                ""
            };
            // For income receipts revenue is credited: shares are negative of money amount.
            out.push(plc(l.account.clone(), net, code));
        }
        if !tax.is_zero() {
            if r.receipt_type == "income" {
                let (acc, code) = ctx.ust_account(l.tax_rate);
                out.push(plt(acc, tax, code, net));
            } else {
                out.push(plt(ctx.vst_account(l.tax_rate), tax, "VST", net));
            }
        }
    }
    out
}

/// Reverse charge / intra-EU acquisition tax lines (Vorsteuer + Umsatzsteuer).
fn reverse_charge_lines(ctx: &Ctx, r: &Receipt, lines: &[ReceiptLine]) -> Vec<PL> {
    let mut out = Vec::new();
    if r.receipt_type != "expense" || r.tax_mode == "standard" {
        return out;
    }
    let mut by_rate: HashMap<Decimal, Decimal> = HashMap::new();
    for l in lines {
        *by_rate.entry(l.tax_rate.normalize()).or_default() += l.net;
    }
    for (rate, net) in by_rate {
        let tax = r2(net * rate / HUNDRED);
        if tax.is_zero() {
            continue;
        }
        let nineteen = rate == Decimal::from(19);
        let (vst, ust, ucode, vcode): (String, String, &'static str, &'static str) = match r.tax_mode.as_str() {
            "eu_acquisition" => (
                ctx.role(if nineteen { "vst_ige19" } else { "vst_ige" }),
                ctx.role(if nineteen { "ust_ige19" } else { "ust_ige" }),
                if nineteen { "UST_IGE19" } else { "UST_IGE7" },
                "VST_IGE",
            ),
            "rc_eu" => (
                ctx.role(if nineteen { "vst13b19" } else { "vst13b" }),
                ctx.role(if nineteen { "ust13b19" } else { "ust13b" }),
                "UST_RC_EU",
                "VST13B",
            ),
            _ => (
                ctx.role(if nineteen { "vst13b19" } else { "vst13b" }),
                ctx.role(if nineteen { "ust13b19" } else { "ust13b" }),
                "UST_RC_DE",
                "VST13B",
            ),
        };
        out.push(plt(ust, -tax, ucode, net));
        if ctx.small_business {
            // No input tax deduction: the tax becomes part of the costs.
            let acc = lines.first().map(|l| l.account.clone()).unwrap_or_else(|| ctx.role("sonst_aufw"));
            out.push(pl(acc, tax));
        } else {
            out.push(plt(vst, tax, vcode, net));
        }
    }
    out
}

fn build(conn: &Connection, ctx: &Ctx, src: Source) -> Result<Vec<EntryDraft>> {
    let mut out = Vec::new();
    match src {
        Source::Document(id) => {
            let Some(doc) = db::find::<Document>(conn, id)? else { return Ok(out) };
            if ctx.euer || !doc.is_invoice_like() || doc.status == "draft" {
                return Ok(out);
            }
            let (parts, gross) = doc_parts(ctx, conn, &doc)?;
            let sign = doc.sign();
            let mut lines = vec![pl(ctx.role("ford"), sign * gross)];
            for p in &parts {
                lines.push(plc(p.account.clone(), -sign * p.net, p.code));
                if !p.tax.is_zero() {
                    lines.push(plt(p.tax_account.clone(), -sign * p.tax, p.tax_code, -sign * p.net));
                }
            }
            out.push(EntryDraft {
                date: doc.date,
                description: format!(
                    "{} {}",
                    crate::services::documents::type_label(&doc.doc_type, &doc.kind),
                    doc.address.lines().next().unwrap_or_default()
                ),
                doc_ref: doc.number.clone(),
                contact_id: doc.contact_id,
                lines,
            });
        }
        Source::Receipt(id) => {
            let Some(r) = db::find::<Receipt>(conn, id)? else { return Ok(out) };
            let lines = receipt_parts(conn, &r)?;
            let name = receipt_party(conn, &r)?;
            let rc = reverse_charge_lines(ctx, &r, &lines);
            if !ctx.euer {
                // Booked at document date against Verbindlichkeiten / Forderungen.
                let counter = if r.receipt_type == "income" { ctx.role("ford") } else { ctx.role("vbl") };
                let money = r.sign() * r.gross_total; // expense: negative
                let mut pls = vec![pl(counter, money)];
                pls.extend(receipt_cost_lines(ctx, &r, &lines, money));
                pls.extend(rc);
                out.push(EntryDraft {
                    date: r.date,
                    description: format!("{} {}", if r.receipt_type == "income" { "Einnahme" } else { "Beleg" }, name),
                    doc_ref: r.number.clone(),
                    contact_id: r.contact_id,
                    lines: pls,
                });
            } else if !rc.is_empty() {
                out.push(EntryDraft {
                    date: r.date,
                    description: format!("Steuerschuldnerschaft § 13b / ig. Erwerb {name}"),
                    doc_ref: r.number.clone(),
                    contact_id: r.contact_id,
                    lines: rc,
                });
            }
        }
        Source::Transaction(id) => {
            let Some(tx) = db::find::<Transaction>(conn, id)? else { return Ok(out) };
            let Some(acc) = db::find::<PaymentAccount>(conn, tx.account_id)? else { return Ok(out) };
            let allocs = db::query::<Allocation>(conn, "WHERE transaction_id = ?1 ORDER BY id", &[v(id)])?;
            for a in allocs {
                let mut lines = vec![pl(acc.ledger_account.clone(), a.amount)];
                let mut description = tx.purpose.clone();
                let mut doc_ref = String::new();
                let mut contact_id = None;
                match a.kind.as_str() {
                    "document" => {
                        let Some(doc) = a.document_id.and_then(|d| db::find::<Document>(conn, d).ok().flatten()) else { continue };
                        doc_ref = doc.number.clone();
                        contact_id = doc.contact_id;
                        description = format!("Zahlung {} {}", doc.number, doc.address.lines().next().unwrap_or_default());
                        if ctx.euer {
                            // Revenue and VAT arise on payment (Zuflussprinzip). The proportional
                            // split carries the sign of the money flow (refunds of credit notes
                            // are negative and therefore reduce revenue).
                            let (parts, gross) = doc_parts(ctx, conn, &doc)?;
                            lines.extend(split_payment_doc(&parts, gross, a.amount));
                        } else {
                            lines.push(pl(ctx.role("ford"), -a.amount));
                        }
                    }
                    "receipt" => {
                        let Some(r) = a.receipt_id.and_then(|d| db::find::<Receipt>(conn, d).ok().flatten()) else { continue };
                        doc_ref = r.number.clone();
                        contact_id = r.contact_id;
                        description = format!("Zahlung {}", receipt_party(conn, &r)?);
                        if ctx.euer {
                            let rl = receipt_parts(conn, &r)?;
                            lines.extend(receipt_cost_lines(ctx, &r, &rl, a.amount));
                        } else {
                            let counter = if r.receipt_type == "income" { ctx.role("ford") } else { ctx.role("vbl") };
                            lines.push(pl(counter, -a.amount));
                        }
                    }
                    _ => {
                        // Direct booking on an account (with optional VAT).
                        if !a.description.is_empty() {
                            description = a.description.clone();
                        }
                        let rate = a.tax_rate;
                        let target = if a.account.is_empty() { ctx.role("sonst_aufw") } else { a.account.clone() };
                        if rate > Decimal::ZERO && !ctx.small_business {
                            let net = r2(a.amount / (Decimal::ONE + rate / HUNDRED));
                            let tax = a.amount - net;
                            if a.amount > Decimal::ZERO {
                                let (tacc, tcode) = ctx.ust_account(rate);
                                let code = if ctx.is_revenue(&target) { base_code_for_rate(rate) } else { "" };
                                lines.push(plc(target, -net, code));
                                lines.push(plt(tacc, -tax, tcode, -net));
                            } else {
                                lines.push(pl(target, -net));
                                lines.push(plt(ctx.vst_account(rate), -tax, "VST", -net));
                            }
                        } else {
                            let code = if ctx.is_revenue(&target) && a.amount > Decimal::ZERO {
                                let c = ctx.account_code(&target);
                                if c.is_empty() && rate.is_zero() && !ctx.small_business { "" } else { c }
                            } else {
                                ""
                            };
                            let code = if ctx.small_business && ctx.is_revenue(&target) { "KU" } else { code };
                            lines.push(plc(target, -a.amount, code));
                        }
                    }
                }
                if tx.reversal_of.is_some() {
                    // Keep the "Storno Nr. …" marker of the reversal visible in the journal.
                    description = tx.purpose.clone();
                }
                if description.trim().is_empty() {
                    description = tx.counterparty.clone();
                }
                if doc_ref.is_empty() {
                    if let Some(n) = tx.cash_number {
                        doc_ref = format!("Kasse {n}");
                    }
                }
                out.push(EntryDraft { date: tx.date, description, doc_ref, contact_id, lines });
            }
        }
        Source::Writeoff(id) => {
            if ctx.euer {
                return Ok(out);
            }
            let Some(a) = db::find::<Allocation>(conn, id)? else { return Ok(out) };
            if a.kind != "writeoff" {
                return Ok(out);
            }
            let Some(doc) = a.document_id.and_then(|d| db::find::<Document>(conn, d).ok().flatten()) else { return Ok(out) };
            // The receivable is cleared; revenue and VAT are reduced (§ 17 UStG correction),
            // i.e. the inverse of a payment split.
            let (parts, gross) = doc_parts(ctx, conn, &doc)?;
            let mut lines = vec![pl(ctx.role("ford"), -a.amount)];
            for mut s in split_payment_doc(&parts, gross, a.amount) {
                if !s.code.starts_with("UST") {
                    let target = match (a.writeoff_reason.as_str(), s.code) {
                        ("skonto", "S19") => Some("skonto19"),
                        ("skonto", "S7") => Some("skonto7"),
                        ("skonto", _) => None,
                        _ => Some("ford_verlust"),
                    };
                    if let Some(r) = target {
                        s.account = ctx.role(r);
                    }
                }
                s.amount = -s.amount;
                s.base = -s.base;
                lines.push(s);
            }
            out.push(EntryDraft {
                date: a.date,
                description: format!(
                    "{} {}",
                    if a.writeoff_reason == "skonto" { "Skonto" } else { "Forderungsausbuchung" },
                    doc.number
                ),
                doc_ref: doc.number.clone(),
                contact_id: doc.contact_id,
                lines,
            });
        }
        Source::Asset(id) => {
            let Some(asset) = db::find::<Asset>(conn, id)? else { return Ok(out) };
            let acc = ctx.accounts.get(&asset.account).cloned();
            let afa_role = match asset.method.as_str() {
                "gwg" => "gwg",
                "pool" => "afa_pool",
                _ => {
                    if acc.map(|a| a.bilanz == "a_immat").unwrap_or(false) { "afa_immat" } else { "afa" }
                }
            };
            let current_year = today().year();
            for row in assets::schedule(&asset) {
                if row.year > current_year {
                    break;
                }
                let is_disp_year = asset.disposal_date.map(|d| d.year() == row.year).unwrap_or(false);
                let date = if is_disp_year { asset.disposal_date.unwrap() } else { ymd(row.year, 12, 31) };
                if row.afa > Decimal::ZERO {
                    out.push(EntryDraft {
                        date,
                        description: format!("AfA {} {}", row.year, asset.name),
                        doc_ref: asset.number.clone(),
                        contact_id: None,
                        lines: vec![pl(ctx.role(afa_role), row.afa), pl(asset.account.clone(), -row.afa)],
                    });
                }
                if row.disposal > Decimal::ZERO {
                    out.push(EntryDraft {
                        date,
                        description: format!("Anlagenabgang {}", asset.name),
                        doc_ref: asset.number.clone(),
                        contact_id: None,
                        lines: vec![pl(ctx.role("abgang"), row.disposal), pl(asset.account.clone(), -row.disposal)],
                    });
                }
            }
        }
        Source::Manual(id) => {
            let Some(m) = db::find::<ManualEntry>(conn, id)? else { return Ok(out) };
            let g = m.amount;
            let mut lines = Vec::new();
            match m.tax_key.as_str() {
                "VST19" | "VST7" => {
                    let rate = if m.tax_key == "VST19" { Decimal::from(19) } else { Decimal::from(7) };
                    let net = r2(g / (Decimal::ONE + rate / HUNDRED));
                    lines.push(pl(m.debit_account.clone(), net));
                    lines.push(plt(ctx.vst_account(rate), g - net, "VST", net));
                    lines.push(pl(m.credit_account.clone(), -g));
                }
                "UST19" | "UST7" => {
                    let rate = if m.tax_key == "UST19" { Decimal::from(19) } else { Decimal::from(7) };
                    let net = r2(g / (Decimal::ONE + rate / HUNDRED));
                    let (tacc, tcode) = ctx.ust_account(rate);
                    lines.push(pl(m.debit_account.clone(), g));
                    let code = if ctx.is_revenue(&m.credit_account) { base_code_for_rate(rate) } else { "" };
                    lines.push(plc(m.credit_account.clone(), -net, code));
                    lines.push(plt(tacc, -(g - net), tcode, -net));
                }
                _ => {
                    lines.push(pl(m.debit_account.clone(), g));
                    let code = if ctx.is_revenue(&m.credit_account) { ctx.account_code(&m.credit_account) } else { "" };
                    lines.push(plc(m.credit_account.clone(), -g, code));
                }
            }
            out.push(EntryDraft { date: m.date, description: m.description.clone(), doc_ref: m.doc_ref.clone(), contact_id: None, lines });
        }
        Source::Opening(id) => {
            let Some(acc) = db::find::<PaymentAccount>(conn, id)? else { return Ok(out) };
            if !acc.opening_balance.is_zero() {
                out.push(EntryDraft {
                    date: acc.opening_date,
                    description: format!("Anfangsbestand {}", acc.name),
                    doc_ref: "EB".into(),
                    contact_id: None,
                    lines: vec![
                        pl(acc.ledger_account.clone(), acc.opening_balance),
                        pl(ctx.role("saldenvortrag"), -acc.opening_balance),
                    ],
                });
            }
        }
    }
    Ok(out)
}

fn receipt_party(conn: &Connection, r: &Receipt) -> Result<String> {
    if let Some(cid) = r.contact_id
        && let Some(c) = db::find::<Contact>(conn, cid)?
    {
        return Ok(c.display_name());
    }
    Ok(r.supplier_name.clone())
}

fn write(conn: &Connection, src: Source, drafts: Vec<EntryDraft>) -> Result<()> {
    let (st, sid) = src.key();
    conn.execute("DELETE FROM journal_entries WHERE source_type = ?1 AND source_id = ?2", rusqlite::params![st, sid])?;
    for d in drafts {
        let lines: Vec<PL> = d.lines.into_iter().filter(|l| !l.amount.is_zero()).collect();
        if lines.is_empty() {
            continue;
        }
        let sum: Decimal = lines.iter().map(|l| l.amount).sum();
        if !sum.is_zero() {
            anyhow::bail!("Buchung '{}' ist nicht ausgeglichen (Differenz {sum})", d.description);
        }
        let mut entry = JournalEntry {
            id: 0,
            date: d.date,
            description: d.description,
            doc_ref: d.doc_ref,
            source_type: st.into(),
            source_id: sid,
            contact_id: d.contact_id,
            created_at: crate::util::now_str(),
        };
        let eid = db::insert(conn, &mut entry)?;
        for l in lines {
            let mut jl = JournalLine {
                id: 0,
                entry_id: eid,
                account: l.account,
                debit: if l.amount > Decimal::ZERO { l.amount } else { Decimal::ZERO },
                credit: if l.amount < Decimal::ZERO { -l.amount } else { Decimal::ZERO },
                tax_code: l.code.into(),
                base: l.base,
            };
            db::insert(conn, &mut jl)?;
        }
    }
    Ok(())
}

pub fn rebuild(conn: &Connection, src: Source) -> Result<()> {
    let ctx = Ctx::load(conn)?;
    let drafts = build(conn, &ctx, src)?;
    write(conn, src, drafts)
}

/// Rebuilds a receipt and every transaction paying it.
pub fn rebuild_receipt_with_payments(conn: &Connection, receipt_id: i64) -> Result<()> {
    rebuild(conn, Source::Receipt(receipt_id))?;
    let mut stmt = conn.prepare("SELECT DISTINCT transaction_id FROM allocations WHERE receipt_id = ?1 AND transaction_id IS NOT NULL")?;
    let ids: Vec<i64> = stmt.query_map([receipt_id], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
    for t in ids {
        rebuild(conn, Source::Transaction(t))?;
    }
    Ok(())
}

pub fn rebuild_all(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM journal_entries", [])?;
    let ids = |sql: &str| -> Result<Vec<i64>> {
        let mut stmt = conn.prepare(sql)?;
        let r = stmt.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
        Ok(r)
    };
    for id in ids("SELECT id FROM payment_accounts")? {
        rebuild(conn, Source::Opening(id))?;
    }
    for id in ids("SELECT id FROM documents WHERE status != 'draft'")? {
        rebuild(conn, Source::Document(id))?;
    }
    for id in ids("SELECT id FROM receipts")? {
        rebuild(conn, Source::Receipt(id))?;
    }
    for id in ids("SELECT id FROM transactions")? {
        rebuild(conn, Source::Transaction(id))?;
    }
    for id in ids("SELECT id FROM allocations WHERE kind = 'writeoff'")? {
        rebuild(conn, Source::Writeoff(id))?;
    }
    for id in ids("SELECT id FROM assets")? {
        rebuild(conn, Source::Asset(id))?;
    }
    for id in ids("SELECT id FROM manual_entries")? {
        rebuild(conn, Source::Manual(id))?;
    }
    Ok(())
}

/// Called once a year rolls over so that the depreciation of the new year exists.
pub fn ensure_current_afa(conn: &Connection) -> Result<()> {
    let year = today().year().to_string();
    if db::meta_get(conn, "afa_year")?.as_deref() != Some(year.as_str()) {
        let mut stmt = conn.prepare("SELECT id FROM assets")?;
        let ids: Vec<i64> = stmt.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
        drop(stmt);
        for id in ids {
            rebuild(conn, Source::Asset(id))?;
        }
        db::meta_set(conn, "afa_year", &year)?;
    }
    Ok(())
}
