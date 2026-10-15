//! Factory defaults: document texts, dunning levels, number ranges, layouts.

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::models::{DocTexts, DunningLevel, LayoutConfig, NumberRange};

fn t(title: &str, intro: &str, closing: &str) -> DocTexts {
    DocTexts { title: title.into(), intro: intro.into(), closing: closing.into() }
}

const GRUSS: &str = "Mit freundlichen Grüßen\n{{Firma}}";

pub fn default_texts() -> BTreeMap<String, DocTexts> {
    let mut m = BTreeMap::new();
    m.insert(
        "quote".into(),
        t(
            "Angebot",
            "{{Anrede}}\n\nvielen Dank für Ihre Anfrage. Gerne unterbreiten wir Ihnen das folgende Angebot:",
            &format!("Dieses Angebot ist gültig bis zum {{{{Gültig bis}}}}. Wir freuen uns auf Ihren Auftrag!\n\n{GRUSS}"),
        ),
    );
    m.insert(
        "order".into(),
        t(
            "Auftragsbestätigung",
            "{{Anrede}}\n\nvielen Dank für Ihren Auftrag. Hiermit bestätigen wir Ihnen die Ausführung der folgenden Leistungen:",
            &format!("Wir freuen uns auf die Zusammenarbeit.\n\n{GRUSS}"),
        ),
    );
    m.insert(
        "delivery".into(),
        t(
            "Lieferschein",
            "{{Anrede}}\n\nwir liefern Ihnen folgende Positionen:",
            "Ware vollständig und in einwandfreiem Zustand erhalten:\n\n\n\n_______________________________________\nDatum, Unterschrift",
        ),
    );
    m.insert(
        "invoice".into(),
        t(
            "Rechnung",
            "{{Anrede}}\n\nvielen Dank für Ihren Auftrag und das damit verbundene Vertrauen. Wir erlauben uns, Ihnen folgende Leistungen in Rechnung zu stellen:",
            &format!(
                "Bitte überweisen Sie den Rechnungsbetrag von {{{{Betrag}}}} unter Angabe der Rechnungsnummer {{{{Nummer}}}} bis zum {{{{Fällig am}}}} auf das unten angegebene Konto.\n\n{GRUSS}"
            ),
        ),
    );
    m.insert(
        "partial".into(),
        t(
            "Abschlagsrechnung",
            "{{Anrede}}\n\nwie vereinbart stellen wir Ihnen folgende Abschlagszahlung in Rechnung:",
            &format!("Bitte überweisen Sie den Betrag von {{{{Betrag}}}} bis zum {{{{Fällig am}}}}.\n\n{GRUSS}"),
        ),
    );
    m.insert(
        "final".into(),
        t(
            "Schlussrechnung",
            "{{Anrede}}\n\nnach Abschluss der Arbeiten stellen wir Ihnen die erbrachten Leistungen abzüglich der bereits geleisteten Abschlagszahlungen in Rechnung:",
            &format!("Bitte überweisen Sie den Restbetrag von {{{{Betrag}}}} bis zum {{{{Fällig am}}}}.\n\n{GRUSS}"),
        ),
    );
    m.insert(
        "cancellation".into(),
        t(
            "Stornorechnung",
            "{{Anrede}}\n\nhiermit stornieren wir die Rechnung {{Bezugsnummer}} vom {{Bezugsdatum}} in voller Höhe.",
            GRUSS,
        ),
    );
    m.insert(
        "credit_note".into(),
        t(
            "Gutschrift",
            "{{Anrede}}\n\nwir schreiben Ihnen folgende Beträge gut:",
            &format!("Der Betrag von {{{{Betrag}}}} wird Ihnen in den nächsten Tagen überwiesen.\n\n{GRUSS}"),
        ),
    );
    m
}

fn lvl(level: i64, name: &str, days: i64, fee: i64, text: &str) -> DunningLevel {
    DunningLevel {
        level,
        name: name.into(),
        days,
        deadline_days: 10,
        fee: Decimal::from(fee),
        title: name.into(),
        text: text.into(),
    }
}

pub fn default_dunning_levels() -> Vec<DunningLevel> {
    vec![
        lvl(
            0,
            "Zahlungserinnerung",
            7,
            0,
            "{{Anrede}}\n\nsicherlich ist es Ihrer Aufmerksamkeit entgangen, dass die nachfolgend aufgeführte Rechnung noch nicht beglichen wurde. Wir bitten Sie, den offenen Betrag von {{Gesamtbetrag}} bis zum {{Neue Frist}} zu überweisen.\n\nSollten Sie die Zahlung bereits veranlasst haben, betrachten Sie dieses Schreiben bitte als gegenstandslos.\n\nMit freundlichen Grüßen\n{{Firma}}",
        ),
        lvl(
            1,
            "1. Mahnung",
            3,
            5,
            "{{Anrede}}\n\nleider konnten wir trotz unserer Zahlungserinnerung bis heute keinen Zahlungseingang für die folgende Rechnung feststellen. Bitte überweisen Sie den Gesamtbetrag von {{Gesamtbetrag}} inklusive Mahngebühren bis spätestens {{Neue Frist}}.\n\nMit freundlichen Grüßen\n{{Firma}}",
        ),
        lvl(
            2,
            "2. Mahnung",
            3,
            10,
            "{{Anrede}}\n\ntrotz unserer bisherigen Mahnung ist die folgende Rechnung weiterhin offen. Wir fordern Sie hiermit nachdrücklich auf, den Gesamtbetrag von {{Gesamtbetrag}} bis zum {{Neue Frist}} zu begleichen.\n\nMit freundlichen Grüßen\n{{Firma}}",
        ),
        lvl(
            3,
            "Letzte Mahnung",
            3,
            15,
            "{{Anrede}}\n\nda Sie auf unsere bisherigen Mahnungen nicht reagiert haben, erhalten Sie hiermit die letzte Mahnung. Sollte der Gesamtbetrag von {{Gesamtbetrag}} nicht bis zum {{Neue Frist}} auf unserem Konto eingegangen sein, werden wir ohne weitere Ankündigung ein gerichtliches Mahnverfahren einleiten bzw. ein Inkassounternehmen beauftragen.\n\nMit freundlichen Grüßen\n{{Firma}}",
        ),
    ]
}

pub fn default_number_ranges() -> Vec<NumberRange> {
    let r = |key: &str, label: &str, format: &str, next: i64, yearly: bool| NumberRange {
        id: 0,
        key: key.into(),
        label: label.into(),
        format: format.into(),
        next,
        year: 0,
        reset_yearly: yearly,
    };
    vec![
        r("invoice", "Rechnungen", "RE-{YYYY}-{NNNN}", 1, true),
        r("credit_note", "Gutschriften", "GS-{YYYY}-{NNNN}", 1, true),
        r("cancellation", "Stornorechnungen", "ST-{YYYY}-{NNNN}", 1, true),
        r("quote", "Angebote", "AN-{YYYY}-{NNNN}", 1, true),
        r("order", "Auftragsbestätigungen", "AB-{YYYY}-{NNNN}", 1, true),
        r("delivery", "Lieferscheine", "LS-{YYYY}-{NNNN}", 1, true),
        r("receipt", "Belege (intern)", "BE-{YYYY}-{NNNN}", 1, true),
        r("customer", "Kundennummern", "{N}", 10001, false),
        r("supplier", "Lieferantennummern", "{N}", 70001, false),
        r("article", "Artikelnummern", "ART-{NNNN}", 1, false),
        r("asset", "Inventarnummern", "INV-{NNNN}", 1, false),
    ]
}

pub fn default_layouts() -> Vec<(String, LayoutConfig)> {
    let classic = LayoutConfig::default();
    let modern = LayoutConfig {
        font: "Inter".into(),
        accent_color: "#0f766e".into(),
        table_style: "striped".into(),
        accent_bar: true,
        show_company_block: true,
        logo_x: 25.0,
        logo_y: 14.0,
        logo_width: 42.0,
        company_x: 130.0,
        company_y: 14.0,
        title_size: 17.0,
        ..LayoutConfig::default()
    };
    let minimal = LayoutConfig {
        font: "IBM Plex Sans".into(),
        accent_color: "#111827".into(),
        table_style: "minimal".into(),
        table_header_fill: false,
        fold_marks: false,
        title_size: 14.0,
        ..LayoutConfig::default()
    };
    vec![
        ("Klassisch (DIN 5008)".into(), classic),
        ("Modern".into(), modern),
        ("Minimalistisch".into(), minimal),
    ]
}
