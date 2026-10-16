// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Kontenrahmen SKR03 / SKR04 (kuratierte Auswahl der gängigsten Konten).
//!
//! Each concept carries both account numbers plus the mapping keys used by the
//! reports (EÜR, GuV nach § 275 HGB, BWA, Bilanz nach § 266 HGB).

use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy)]
pub struct Concept {
    pub role: &'static str,
    pub skr03: &'static str,
    pub skr04: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub euer: &'static str,
    pub bwa: &'static str,
    pub guv: &'static str,
    pub bilanz: &'static str,
    pub rate: i32,
    pub code: &'static str,
    pub auto: bool,
    pub pick: &'static str,
    pub hint: &'static str,
}

#[allow(clippy::too_many_arguments)]
const fn c(
    role: &'static str,
    skr03: &'static str,
    skr04: &'static str,
    name: &'static str,
    kind: &'static str,
    euer: &'static str,
    bwa: &'static str,
    guv: &'static str,
    bilanz: &'static str,
    rate: i32,
    code: &'static str,
    auto: bool,
    pick: &'static str,
    hint: &'static str,
) -> Concept {
    Concept { role, skr03, skr04, name, kind, euer, bwa, guv, bilanz, rate, code, auto, pick, hint }
}

/// Expense account shortcut.
const fn e(
    role: &'static str,
    skr03: &'static str,
    skr04: &'static str,
    name: &'static str,
    euer: &'static str,
    bwa: &'static str,
    guv: &'static str,
    rate: i32,
    hint: &'static str,
) -> Concept {
    c(role, skr03, skr04, name, "expense", euer, bwa, guv, "", rate, "", false, "expense", hint)
}

/// Fixed asset account shortcut.
const fn fa(skr03: &'static str, skr04: &'static str, name: &'static str, bilanz: &'static str, hint: &'static str) -> Concept {
    c("", skr03, skr04, name, "asset", "", "", "", bilanz, 19, "", false, "asset", hint)
}

pub static CONCEPTS: &[Concept] = &[
    // ---------------- Anlagevermögen ----------------
    fa("0027", "0135", "EDV-Software", "a_immat", "Gekaufte Software (Nutzungsdauer i. d. R. 1–3 Jahre)"),
    fa("0210", "0440", "Maschinen", "a_sach", "Maschinen und maschinelle Anlagen"),
    fa("0320", "0520", "Pkw", "a_sach", "Betriebliche Personenkraftwagen"),
    fa("0350", "0540", "Lkw", "a_sach", "Lastkraftwagen"),
    fa("0440", "0620", "Werkzeuge", "a_sach", "Werkzeuge mit Anschaffungskosten über 800 €"),
    fa("0430", "0640", "Ladeneinrichtung", "a_sach", ""),
    fa("0420", "0650", "Büroeinrichtung", "a_sach", "Büromöbel, Computer, Drucker"),
    fa("0480", "0670", "Geringwertige Wirtschaftsgüter", "a_sach", "GWG bis 800 € netto, Sofortabschreibung"),
    fa("0485", "0675", "Wirtschaftsgüter (Sammelposten)", "a_sach", "250,01 € bis 1.000 € netto, Abschreibung über 5 Jahre"),
    fa("0490", "0690", "Sonstige Betriebs- und Geschäftsausstattung", "a_sach", ""),
    // ---------------- Umlaufvermögen ----------------
    c("kasse", "1000", "1600", "Kasse", "current", "", "", "", "a_liquide", -1, "", false, "cash", ""),
    c("bank", "1200", "1800", "Bank", "current", "", "", "", "a_liquide", -1, "", false, "bank", ""),
    c("bank2", "1210", "1810", "Bank 2", "current", "", "", "", "a_liquide", -1, "", false, "bank", ""),
    c("paypal", "1220", "1820", "Zahlungsdienstleister (PayPal, Stripe)", "current", "", "", "", "a_liquide", -1, "", false, "bank", ""),
    c("transit", "1360", "1460", "Geldtransit", "current", "", "", "", "a_sonst_vg", -1, "", false, "transfer", "Umbuchungen zwischen eigenen Konten (z. B. Bank → Kasse)"),
    c("ford", "1400", "1200", "Forderungen aus Lieferungen und Leistungen", "current", "", "", "", "a_ford", -1, "", false, "", ""),
    c("", "1500", "1300", "Sonstige Vermögensgegenstände", "current", "", "", "", "a_sonst_vg", -1, "", false, "other", ""),
    c("", "1590", "1370", "Durchlaufende Posten", "current", "", "", "", "a_sonst_vg", -1, "", false, "other", "Gelder, die im fremden Namen vereinnahmt und verausgabt werden"),
    c("", "3980", "1140", "Bestand Waren", "current", "", "", "", "a_vorraete", -1, "", false, "other", ""),
    c("", "0980", "1900", "Aktive Rechnungsabgrenzung", "current", "", "", "", "a_rap", -1, "", false, "other", ""),
    // ---------------- Steuern ----------------
    c("vst", "1570", "1400", "Abziehbare Vorsteuer", "tax", "ba_vst", "", "", "tax", -1, "", false, "", ""),
    c("vst7", "1571", "1401", "Abziehbare Vorsteuer 7 %", "tax", "ba_vst", "", "", "tax", 7, "", false, "", ""),
    c("vst19", "1576", "1406", "Abziehbare Vorsteuer 19 %", "tax", "ba_vst", "", "", "tax", 19, "", false, "", ""),
    c("vst_ige", "1572", "1402", "Abziehbare Vorsteuer aus innergemeinschaftlichem Erwerb", "tax", "ba_vst", "", "", "tax", -1, "", false, "", ""),
    c("vst_ige19", "1574", "1404", "Abziehbare Vorsteuer aus innergemeinschaftlichem Erwerb 19 %", "tax", "ba_vst", "", "", "tax", 19, "", false, "", ""),
    c("vst13b", "1578", "1408", "Abziehbare Vorsteuer nach § 13b UStG", "tax", "ba_vst", "", "", "tax", -1, "", false, "", ""),
    c("vst13b19", "1577", "1407", "Abziehbare Vorsteuer nach § 13b UStG 19 %", "tax", "ba_vst", "", "", "tax", 19, "", false, "", ""),
    c("eust", "1588", "1433", "Entstandene Einfuhrumsatzsteuer", "tax", "ba_vst", "", "", "tax", -1, "", false, "tax", ""),
    c("ust", "1770", "3800", "Umsatzsteuer", "tax", "be_ust_vereinnahmt", "", "", "tax", -1, "", false, "", ""),
    c("ust7", "1771", "3801", "Umsatzsteuer 7 %", "tax", "be_ust_vereinnahmt", "", "", "tax", 7, "", false, "", ""),
    c("ust19", "1776", "3806", "Umsatzsteuer 19 %", "tax", "be_ust_vereinnahmt", "", "", "tax", 19, "", false, "", ""),
    c("ust_ige", "1772", "3802", "Umsatzsteuer aus innergemeinschaftlichem Erwerb", "tax", "be_ust_vereinnahmt", "", "", "tax", -1, "", false, "", ""),
    c("ust_ige19", "1774", "3804", "Umsatzsteuer aus innergemeinschaftlichem Erwerb 19 %", "tax", "be_ust_vereinnahmt", "", "", "tax", 19, "", false, "", ""),
    c("ust13b", "1785", "3835", "Umsatzsteuer nach § 13b UStG", "tax", "be_ust_vereinnahmt", "", "", "tax", -1, "", false, "", ""),
    c("ust13b19", "1787", "3837", "Umsatzsteuer nach § 13b UStG 19 %", "tax", "be_ust_vereinnahmt", "", "", "tax", 19, "", false, "", ""),
    c("ust_vz", "1780", "3820", "Umsatzsteuer-Vorauszahlungen", "tax", "ba_ust_fa", "", "", "tax", -1, "", false, "tax", "Zahlungen an das Finanzamt laut Umsatzsteuer-Voranmeldung"),
    c("ust_vj", "1790", "3841", "Umsatzsteuer Vorjahr", "tax", "ba_ust_fa", "", "", "tax", -1, "", false, "tax", "Nachzahlungen/Erstattungen aus der USt-Jahreserklärung"),
    // ---------------- Verbindlichkeiten, Rückstellungen ----------------
    c("vbl", "1600", "3300", "Verbindlichkeiten aus Lieferungen und Leistungen", "liability", "", "", "", "p_vb_ll", -1, "", false, "", ""),
    c("", "1700", "3500", "Sonstige Verbindlichkeiten", "liability", "", "", "", "p_vb_sonst", -1, "", false, "other", ""),
    c("darlehen", "0630", "3150", "Verbindlichkeiten gegenüber Kreditinstituten", "liability", "", "", "", "p_vb_kredit", -1, "", false, "other", "Darlehen, Kredite"),
    c("", "1740", "3720", "Verbindlichkeiten aus Lohn und Gehalt", "liability", "", "", "", "p_vb_sonst", -1, "", false, "other", ""),
    c("", "1741", "3730", "Verbindlichkeiten aus Lohn- und Kirchensteuer", "liability", "", "", "", "p_vb_sonst", -1, "", false, "other", ""),
    c("", "1742", "3740", "Verbindlichkeiten im Rahmen der sozialen Sicherheit", "liability", "", "", "", "p_vb_sonst", -1, "", false, "other", ""),
    c("anzahlungen", "1710", "3250", "Erhaltene Anzahlungen auf Bestellungen", "liability", "", "", "", "p_anz", -1, "", false, "other", ""),
    c("", "0970", "3070", "Sonstige Rückstellungen", "liability", "", "", "", "p_rueck", -1, "", false, "other", ""),
    c("", "0977", "3095", "Rückstellungen für Abschluss- und Prüfungskosten", "liability", "", "", "", "p_rueck", -1, "", false, "other", ""),
    c("", "0956", "3035", "Gewerbesteuerrückstellung", "liability", "", "", "", "p_rueck", -1, "", false, "other", ""),
    c("", "0990", "3900", "Passive Rechnungsabgrenzung", "liability", "", "", "", "p_rap", -1, "", false, "other", ""),
    // ---------------- Eigenkapital & Privat ----------------
    c("", "0800", "2900", "Gezeichnetes Kapital", "equity", "", "", "", "p_ek", -1, "", false, "other", ""),
    c("kapital", "0880", "2010", "Variables Kapital", "equity", "", "", "", "p_ek", -1, "", false, "other", ""),
    c("", "0860", "2970", "Gewinnvortrag vor Verwendung", "equity", "", "", "", "p_ek", -1, "", false, "other", ""),
    c("", "0868", "2978", "Verlustvortrag vor Verwendung", "equity", "", "", "", "p_ek", -1, "", false, "other", ""),
    c("privat_ent", "1800", "2100", "Privatentnahmen allgemein", "private", "", "", "", "p_ek", -1, "", false, "private", "Geld für private Zwecke entnommen"),
    c("", "1810", "2150", "Privatsteuern", "private", "", "", "", "p_ek", -1, "", false, "private", "Einkommensteuer, Kirchensteuer, Solidaritätszuschlag"),
    c("privat_ein", "1890", "2180", "Privateinlagen", "private", "", "", "", "p_ek", -1, "", false, "private", "Private Mittel in das Unternehmen eingelegt"),
    c("saldenvortrag", "9000", "9000", "Saldenvorträge, Sachkonten", "neutral", "", "", "", "p_ek", -1, "", false, "", "Eröffnungsbuchungen"),
    // ---------------- Erlöse ----------------
    c("erl19", "8400", "4400", "Erlöse 19 % USt", "revenue", "be_ust", "umsatz", "g01", "", 19, "S19", true, "revenue", ""),
    c("erl7", "8300", "4300", "Erlöse 7 % USt", "revenue", "be_ust", "umsatz", "g01", "", 7, "S7", true, "revenue", ""),
    c("erl", "8200", "4200", "Erlöse", "revenue", "be_ust", "umsatz", "g01", "", -1, "", false, "revenue", ""),
    c("erl_frei", "8100", "4100", "Steuerfreie Umsätze § 4 Nr. 8 ff. UStG", "revenue", "be_frei", "umsatz", "g01", "", 0, "FREI", false, "revenue", ""),
    c("erl_ku", "8195", "4185", "Erlöse als Kleinunternehmer i. S. d. § 19 Abs. 1 UStG", "revenue", "be_ku", "umsatz", "g01", "", 0, "KU", false, "revenue", ""),
    c("erl_igl", "8125", "4125", "Steuerfreie innergemeinschaftliche Lieferungen § 4 Nr. 1b UStG", "revenue", "be_frei", "umsatz", "g01", "", 0, "IGL", false, "revenue", ""),
    c("erl_export", "8120", "4120", "Steuerfreie Umsätze § 4 Nr. 1a UStG (Ausfuhr Drittland)", "revenue", "be_frei", "umsatz", "g01", "", 0, "EXP", false, "revenue", ""),
    c("erl_eu_rc", "8336", "4336", "Erlöse aus im anderen EU-Land steuerpflichtigen sonstigen Leistungen (Reverse Charge)", "revenue", "be_frei", "umsatz", "g01", "", 0, "EURC", false, "revenue", ""),
    c("erl_13b", "8337", "4337", "Erlöse aus Leistungen nach § 13b UStG", "revenue", "be_ust", "umsatz", "g01", "", 0, "RC13B_OUT", false, "revenue", ""),
    c("erl_nstb", "8338", "4338", "Erlöse aus im Drittland steuerbaren Leistungen", "revenue", "be_frei", "umsatz", "g01", "", 0, "NSTB", false, "revenue", ""),
    c("skonto19", "8736", "4736", "Gewährte Skonti 19 % USt", "revenue", "be_ust", "umsatz", "g01", "", 19, "S19", true, "", ""),
    c("skonto7", "8731", "4731", "Gewährte Skonti 7 % USt", "revenue", "be_ust", "umsatz", "g01", "", 7, "S7", true, "", ""),
    c("erl_anlagen", "8820", "4845", "Erlöse aus Verkäufen Sachanlagevermögen 19 % USt", "revenue", "be_anlagen", "sonst_erloese", "g04", "", 19, "S19", false, "revenue", ""),
    c("sonst_ertr", "2700", "4830", "Sonstige betriebliche Erträge", "revenue", "be_frei", "neutral_ertr", "g04", "", 0, "", false, "revenue", "z. B. Mahngebühren, Versicherungserstattungen"),
    c("zinsertr", "2650", "7100", "Sonstige Zinsen und ähnliche Erträge", "revenue", "be_frei", "zinsertr", "g11", "", 0, "", false, "revenue", ""),
    // ---------------- Wareneinsatz ----------------
    e("", "3200", "5200", "Wareneingang", "ba_waren", "material", "g05a", -1, ""),
    e("", "3300", "5300", "Wareneingang 7 % Vorsteuer", "ba_waren", "material", "g05a", 7, ""),
    e("", "3400", "5400", "Wareneingang 19 % Vorsteuer", "ba_waren", "material", "g05a", 19, "Einkauf von Handelswaren"),
    e("", "3100", "5900", "Fremdleistungen", "ba_fremd", "fremd", "g05b", 19, "Subunternehmer, Freelancer"),
    // ---------------- Personal ----------------
    e("", "4110", "6010", "Löhne", "ba_personal", "personal", "g06a", 0, ""),
    e("", "4120", "6020", "Gehälter", "ba_personal", "personal", "g06a", 0, ""),
    e("", "4190", "6030", "Aushilfslöhne", "ba_personal", "personal", "g06a", 0, "Minijobs"),
    e("", "4130", "6110", "Gesetzliche soziale Aufwendungen", "ba_personal", "personal", "g06b", 0, "Arbeitgeberanteile zur Sozialversicherung"),
    // ---------------- Raumkosten ----------------
    e("", "4200", "6305", "Raumkosten", "ba_raum", "raum", "g08", 19, ""),
    e("", "4210", "6310", "Miete (unbewegliche Wirtschaftsgüter)", "ba_raum", "raum", "g08", 0, "Büro-, Lager-, Ladenmiete"),
    e("", "4240", "6325", "Gas, Strom, Wasser", "ba_raum", "raum", "g08", 19, ""),
    e("", "4250", "6330", "Reinigung", "ba_raum", "raum", "g08", 19, ""),
    e("", "4260", "6335", "Instandhaltung betrieblicher Räume", "ba_raum", "raum", "g08", 19, ""),
    // ---------------- Versicherungen, Beiträge, Steuern ----------------
    e("", "4360", "6400", "Versicherungen", "ba_versicherung", "versich", "g08", 0, "Betriebliche Versicherungen (Haftpflicht, Inhalt)"),
    e("", "4380", "6420", "Beiträge", "ba_versicherung", "versich", "g08", 0, "IHK, Kammern, Verbände"),
    e("", "4390", "6430", "Sonstige Abgaben", "ba_versicherung", "versich", "g08", 0, ""),
    e("", "4320", "7610", "Gewerbesteuer", "nabz", "steuern_ee", "g14", 0, "Nicht als Betriebsausgabe abziehbar"),
    // ---------------- Fahrzeugkosten ----------------
    e("", "4500", "6500", "Fahrzeugkosten", "ba_kfz", "kfz", "g08", 19, ""),
    e("", "4510", "7685", "Kfz-Steuer", "ba_kfz", "steuern_betr", "g16", 0, ""),
    e("", "4520", "6520", "Kfz-Versicherungen", "ba_kfz", "kfz", "g08", 0, ""),
    e("", "4530", "6530", "Laufende Kfz-Betriebskosten", "ba_kfz", "kfz", "g08", 19, "Kraftstoff, Pflege, Parkgebühren"),
    e("", "4540", "6540", "Kfz-Reparaturen", "ba_kfz", "kfz", "g08", 19, ""),
    e("", "4580", "6570", "Sonstige Kfz-Kosten", "ba_kfz", "kfz", "g08", 19, ""),
    // ---------------- Werbe- und Reisekosten ----------------
    e("", "4600", "6600", "Werbekosten", "ba_werbung", "werbe_reise", "g08", 19, "Anzeigen, Online-Marketing, Drucksachen"),
    e("", "4630", "6610", "Geschenke abzugsfähig", "ba_geschenke", "werbe_reise", "g08", 19, "Bis 50 € netto pro Person und Jahr"),
    e("", "4635", "6620", "Geschenke nicht abzugsfähig", "nabz", "werbe_reise", "g08", 19, ""),
    e("", "4650", "6640", "Bewirtungskosten", "ba_bewirtung", "werbe_reise", "g08", 19, "Abziehbarer Anteil (70 %) geschäftlicher Bewirtungen"),
    e("", "4654", "6644", "Nicht abzugsfähige Bewirtungskosten", "nabz", "werbe_reise", "g08", 19, "Nicht abziehbarer Anteil (30 %)"),
    e("", "4660", "6650", "Reisekosten Arbeitnehmer", "ba_reise", "werbe_reise", "g08", 19, ""),
    e("", "4670", "6670", "Reisekosten Unternehmer", "ba_reise", "werbe_reise", "g08", 19, ""),
    e("", "4673", "6673", "Reisekosten Unternehmer Fahrtkosten", "ba_reise", "werbe_reise", "g08", 19, "Bahn, Flug, Taxi"),
    e("", "4674", "6674", "Reisekosten Unternehmer Verpflegungsmehraufwand", "ba_reise", "werbe_reise", "g08", 0, "Pauschalen"),
    e("", "4676", "6680", "Reisekosten Unternehmer Übernachtungsaufwand", "ba_reise", "werbe_reise", "g08", 7, "Hotel"),
    // ---------------- Abschreibungen ----------------
    e("afa_immat", "4822", "6200", "Abschreibungen auf immaterielle Vermögensgegenstände", "ba_afa", "afa", "g07", 0, ""),
    e("afa", "4830", "6220", "Abschreibungen auf Sachanlagen", "ba_afa", "afa", "g07", 0, ""),
    e("gwg", "4855", "6260", "Sofortabschreibung geringwertiger Wirtschaftsgüter", "ba_gwg", "afa", "g07", 19, "GWG bis 800 € netto direkt als Aufwand"),
    e("afa_pool", "4862", "6264", "Abschreibungen auf den Sammelposten Wirtschaftsgüter", "ba_gwg", "afa", "g07", 0, ""),
    e("abgang", "2310", "6895", "Anlagenabgänge Sachanlagen (Restbuchwert bei Buchverlust)", "ba_restbuchwert", "neutral_aufw", "g08", 0, ""),
    // ---------------- Reparaturen ----------------
    e("", "4800", "6460", "Reparaturen und Instandhaltung von technischen Anlagen und Maschinen", "ba_sonst", "reparatur", "g08", 19, ""),
    e("", "4805", "6470", "Reparaturen und Instandhaltung von Betriebs- und Geschäftsausstattung", "ba_sonst", "reparatur", "g08", 19, ""),
    // ---------------- Sonstige Kosten ----------------
    e("", "4810", "6840", "Mietleasing (bewegliche Wirtschaftsgüter)", "ba_miete_bew", "sonstige", "g08", 19, "Leasing von Geräten"),
    e("", "4960", "6835", "Mieten für Einrichtungen", "ba_miete_bew", "sonstige", "g08", 19, ""),
    e("sonst_aufw", "4900", "6300", "Sonstige betriebliche Aufwendungen", "ba_sonst", "sonstige", "g08", 19, ""),
    e("", "4910", "6800", "Porto", "ba_sonst", "sonstige", "g08", 0, "Briefmarken, Versandkosten"),
    e("", "4920", "6805", "Telefon", "ba_telekom", "sonstige", "g08", 19, "Festnetz, Mobilfunk"),
    e("", "4925", "6810", "Telefax und Internetkosten", "ba_telekom", "sonstige", "g08", 19, "Internetanschluss, Hosting, Domains"),
    e("", "4930", "6815", "Bürobedarf", "ba_sonst", "sonstige", "g08", 19, "Papier, Druckerpatronen, Büromaterial"),
    e("", "4940", "6820", "Zeitschriften, Bücher (Fachliteratur)", "ba_sonst", "sonstige", "g08", 7, ""),
    e("", "4945", "6821", "Fortbildungskosten", "ba_fortbildung", "sonstige", "g08", 19, "Seminare, Kurse, Konferenzen"),
    e("", "4950", "6825", "Rechts- und Beratungskosten", "ba_beratung", "sonstige", "g08", 19, "Anwalt, Steuerberatung"),
    e("", "4955", "6830", "Buchführungskosten", "ba_beratung", "sonstige", "g08", 19, ""),
    e("", "4957", "6827", "Abschluss- und Prüfungskosten", "ba_beratung", "sonstige", "g08", 19, ""),
    e("", "4964", "6837", "Aufwendungen für die zeitlich befristete Überlassung von Rechten (Lizenzen)", "ba_sonst", "sonstige", "g08", 19, "Software-Abonnements, Lizenzen"),
    e("", "4969", "6859", "Aufwendungen für Abraum- und Abfallbeseitigung", "ba_sonst", "sonstige", "g08", 19, ""),
    e("geldverkehr", "4970", "6855", "Nebenkosten des Geldverkehrs", "ba_sonst", "sonstige", "g08", 0, "Kontoführung, Transaktionsgebühren"),
    e("", "4980", "6850", "Sonstiger Betriebsbedarf", "ba_sonst", "sonstige", "g08", 19, ""),
    e("", "4985", "6845", "Werkzeuge und Kleingeräte", "ba_sonst", "sonstige", "g08", 19, ""),
    e("ford_verlust", "2400", "6930", "Forderungsverluste (übliche Höhe)", "ba_sonst", "neutral_aufw", "g08", 0, ""),
    e("zinsaufw", "2100", "7300", "Zinsen und ähnliche Aufwendungen", "ba_schuldzinsen", "zinsaufw", "g13", 0, "Darlehenszinsen, Überziehungszinsen"),
];

pub fn number_of(c: &Concept, chart: &str) -> &'static str {
    if chart == "SKR04" { c.skr04 } else { c.skr03 }
}

/// Account number for a posting role in the given chart.
pub fn role(chart: &str, role: &str) -> String {
    CONCEPTS
        .iter()
        .find(|c| c.role == role)
        .map(|c| number_of(c, chart).to_string())
        .unwrap_or_else(|| panic!("unknown account role {role}"))
}

pub fn rate_opt(c: &Concept) -> Option<Decimal> {
    if c.rate < 0 { None } else { Some(Decimal::from(c.rate)) }
}

pub fn accounts_for(chart: &str) -> Vec<crate::models::Account> {
    CONCEPTS
        .iter()
        .map(|c| crate::models::Account {
            id: 0,
            number: number_of(c, chart).to_string(),
            name: c.name.to_string(),
            kind: c.kind.to_string(),
            euer: c.euer.to_string(),
            guv: c.guv.to_string(),
            bwa: c.bwa.to_string(),
            bilanz: c.bilanz.to_string(),
            tax_rate: rate_opt(c),
            tax_code: c.code.to_string(),
            role: c.role.to_string(),
            datev_auto: c.auto,
            pick: c.pick.to_string(),
            hint: c.hint.to_string(),
            system: !c.role.is_empty(),
            active: true,
        })
        .collect()
}

// --- Labels for report keys -------------------------------------------------

pub const EUER_INCOME: &[(&str, &str)] = &[
    ("be_ku", "Betriebseinnahmen als umsatzsteuerlicher Kleinunternehmer"),
    ("be_ust", "Umsatzsteuerpflichtige Betriebseinnahmen"),
    ("be_frei", "Umsatzsteuerfreie, nicht umsatzsteuerbare Betriebseinnahmen"),
    ("be_ust_vereinnahmt", "Vereinnahmte Umsatzsteuer sowie Umsatzsteuer auf unentgeltliche Wertabgaben"),
    ("be_ust_erstattet", "Vom Finanzamt erstattete und ggf. verrechnete Umsatzsteuer"),
    ("be_anlagen", "Veräußerung oder Entnahme von Anlagevermögen"),
    ("be_entnahmen", "Private Kfz-Nutzung und sonstige Sach-, Nutzungs- und Leistungsentnahmen"),
];

pub const EUER_EXPENSE: &[(&str, &str)] = &[
    ("ba_waren", "Waren, Rohstoffe und Hilfsstoffe einschließlich der Nebenkosten"),
    ("ba_fremd", "Bezogene Fremdleistungen"),
    ("ba_personal", "Ausgaben für eigenes Personal"),
    ("ba_afa", "Absetzung für Abnutzung (AfA)"),
    ("ba_gwg", "Geringwertige Wirtschaftsgüter und Auflösung Sammelposten"),
    ("ba_restbuchwert", "Restbuchwert der ausgeschiedenen Anlagegüter"),
    ("ba_raum", "Raumkosten und sonstige Grundstücksaufwendungen"),
    ("ba_telekom", "Aufwendungen für Telekommunikation"),
    ("ba_reise", "Übernachtungs- und Reisenebenkosten bei Geschäftsreisen"),
    ("ba_fortbildung", "Fortbildungskosten"),
    ("ba_beratung", "Kosten für Rechts- und Steuerberatung, Buchführung"),
    ("ba_miete_bew", "Miete / Leasing für bewegliche Wirtschaftsgüter"),
    ("ba_versicherung", "Beiträge, Gebühren, Abgaben und Versicherungen"),
    ("ba_werbung", "Werbekosten"),
    ("ba_schuldzinsen", "Schuldzinsen"),
    ("ba_kfz", "Kraftfahrzeugkosten und andere Fahrtkosten"),
    ("ba_geschenke", "Geschenke (abziehbar)"),
    ("ba_bewirtung", "Bewirtungsaufwendungen (abziehbarer Teil)"),
    ("ba_vst", "Gezahlte Vorsteuerbeträge"),
    ("ba_ust_fa", "An das Finanzamt gezahlte und ggf. verrechnete Umsatzsteuer"),
    ("ba_sonst", "Übrige unbeschränkt abziehbare Betriebsausgaben"),
];

pub const GUV_LINES: &[(&str, &str)] = &[
    ("g01", "1. Umsatzerlöse"),
    ("g04", "4. Sonstige betriebliche Erträge"),
    ("g05a", "5a. Aufwendungen für Roh-, Hilfs- und Betriebsstoffe und für bezogene Waren"),
    ("g05b", "5b. Aufwendungen für bezogene Leistungen"),
    ("g06a", "6a. Löhne und Gehälter"),
    ("g06b", "6b. Soziale Abgaben und Aufwendungen für Altersversorgung"),
    ("g07", "7. Abschreibungen auf immaterielle Vermögensgegenstände und Sachanlagen"),
    ("g08", "8. Sonstige betriebliche Aufwendungen"),
    ("g11", "11. Sonstige Zinsen und ähnliche Erträge"),
    ("g13", "13. Zinsen und ähnliche Aufwendungen"),
    ("g14", "14. Steuern vom Einkommen und vom Ertrag"),
    ("g16", "16. Sonstige Steuern"),
];

pub const BWA_LINES: &[(&str, &str)] = &[
    ("umsatz", "Umsatzerlöse"),
    ("material", "Material- / Wareneinkauf"),
    ("fremd", "Fremdleistungen"),
    ("sonst_erloese", "Sonstige betriebliche Erlöse"),
    ("personal", "Personalkosten"),
    ("raum", "Raumkosten"),
    ("steuern_betr", "Betriebliche Steuern"),
    ("versich", "Versicherungen / Beiträge"),
    ("kfz", "Kfz-Kosten (ohne Steuer)"),
    ("werbe_reise", "Werbe- / Reisekosten"),
    ("afa", "Abschreibungen"),
    ("reparatur", "Reparatur / Instandhaltung"),
    ("sonstige", "Sonstige Kosten"),
    ("zinsaufw", "Zinsaufwand"),
    ("neutral_aufw", "Sonstiger neutraler Aufwand"),
    ("zinsertr", "Zinserträge"),
    ("neutral_ertr", "Sonstiger neutraler Ertrag"),
    ("steuern_ee", "Steuern Einkommen und Ertrag"),
];

pub const BILANZ_AKTIVA: &[(&str, &str)] = &[
    ("a_immat", "A. I. Immaterielle Vermögensgegenstände"),
    ("a_sach", "A. II. Sachanlagen"),
    ("a_finanz", "A. III. Finanzanlagen"),
    ("a_vorraete", "B. I. Vorräte"),
    ("a_ford", "B. II. 1. Forderungen aus Lieferungen und Leistungen"),
    ("a_sonst_vg", "B. II. 4. Sonstige Vermögensgegenstände"),
    ("a_liquide", "B. IV. Kassenbestand, Guthaben bei Kreditinstituten"),
    ("a_rap", "C. Rechnungsabgrenzungsposten"),
];

pub const BILANZ_PASSIVA: &[(&str, &str)] = &[
    ("p_ek", "A. Eigenkapital"),
    ("p_rueck", "B. Rückstellungen"),
    ("p_vb_kredit", "C. 2. Verbindlichkeiten gegenüber Kreditinstituten"),
    ("p_anz", "C. 3. Erhaltene Anzahlungen auf Bestellungen"),
    ("p_vb_ll", "C. 4. Verbindlichkeiten aus Lieferungen und Leistungen"),
    ("p_vb_sonst", "C. 8. Sonstige Verbindlichkeiten"),
    ("p_rap", "D. Rechnungsabgrenzungsposten"),
];
