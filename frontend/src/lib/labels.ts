// Labels, colors and option lists shared across the UI.

export type DocType = 'quote' | 'order' | 'delivery' | 'invoice' | 'credit_note';

export const DOC_TYPES: Record<DocType, { singular: string; plural: string; route: string; newLabel: string; numberLabel: string }> = {
  quote: { singular: 'Angebot', plural: 'Angebote', route: '/angebote', newLabel: 'Neues Angebot', numberLabel: 'Angebotsnummer' },
  order: { singular: 'Auftragsbestätigung', plural: 'Auftragsbestätigungen', route: '/auftraege', newLabel: 'Neue Auftragsbestätigung', numberLabel: 'Auftragsnummer' },
  delivery: { singular: 'Lieferschein', plural: 'Lieferscheine', route: '/lieferscheine', newLabel: 'Neuer Lieferschein', numberLabel: 'Lieferscheinnummer' },
  invoice: { singular: 'Rechnung', plural: 'Rechnungen', route: '/rechnungen', newLabel: 'Neue Rechnung', numberLabel: 'Rechnungsnummer' },
  credit_note: { singular: 'Gutschrift', plural: 'Gutschriften', route: '/gutschriften', newLabel: 'Neue Gutschrift', numberLabel: 'Gutschriftsnummer' },
};

export function docLabel(docType: string, kind?: string) {
  if (docType === 'invoice') {
    if (kind === 'partial') return 'Abschlagsrechnung';
    if (kind === 'final') return 'Schlussrechnung';
    if (kind === 'cancellation') return 'Stornorechnung';
  }
  return DOC_TYPES[docType as DocType]?.singular ?? 'Dokument';
}

export function statusInfo(status: string, docType?: string, daysOverdue = 0): { label: string; color: string } {
  if (daysOverdue > 0 && (status === 'open' || status === 'partial')) return { label: 'Überfällig', color: 'red' };
  switch (status) {
    case 'draft':
      return { label: 'Entwurf', color: 'gray' };
    case 'open':
      if (docType === 'invoice' || docType === 'credit_note') return { label: 'Offen', color: 'blue' };
      if (docType === 'quote') return { label: 'Versendet', color: 'blue' };
      return { label: 'Offen', color: 'blue' };
    case 'accepted':
      return { label: 'Angenommen', color: 'teal' };
    case 'rejected':
      return { label: 'Abgelehnt', color: 'red' };
    case 'done':
      return { label: docType === 'delivery' ? 'Geliefert' : 'Erledigt', color: 'teal' };
    case 'partial':
      return { label: 'Teilbezahlt', color: 'orange' };
    case 'paid':
      return { label: docType === 'credit_note' ? 'Erstattet' : 'Bezahlt', color: 'teal' };
    case 'cancelled':
      return { label: 'Storniert', color: 'gray' };
    case 'booked':
      return { label: 'Zugeordnet', color: 'teal' };
    default:
      return { label: status, color: 'gray' };
  }
}

export const TAX_MODES = [
  { value: 'standard', label: 'Regelbesteuerung (USt ausweisen)' },
  { value: 'small_business', label: 'Kleinunternehmer (§ 19 UStG)' },
  { value: 'reverse_charge', label: 'Reverse Charge Inland (§ 13b UStG)' },
  { value: 'eu_service', label: 'Sonstige Leistung EU (Reverse Charge)' },
  { value: 'eu_delivery', label: 'Innergemeinschaftliche Lieferung (steuerfrei)' },
  { value: 'export', label: 'Ausfuhrlieferung Drittland (steuerfrei)' },
  { value: 'third_country', label: 'Leistung Drittland (nicht steuerbar)' },
];

export const RECEIPT_TAX_MODES = [
  { value: 'standard', label: 'Normal (Vorsteuer laut Beleg)' },
  { value: 'rc_eu', label: 'Reverse Charge EU (§ 13b Abs. 1 UStG)' },
  { value: 'rc_other', label: 'Reverse Charge Inland/Drittland (§ 13b Abs. 2 UStG)' },
  { value: 'eu_acquisition', label: 'Innergemeinschaftlicher Erwerb' },
];

export const TAX_RATES = [
  { value: '19', label: '19 %' },
  { value: '7', label: '7 %' },
  { value: '0', label: '0 %' },
];

export const UNITS = ['Stk', 'Std', 'Tag', 'Pauschal', 'Monat', 'Woche', 'Jahr', 'km', 'kg', 'm', 'm²', 'l', 'Paket', 'Lizenz', 'Set'];

export const EINVOICE_FORMATS = [
  { value: 'pdf', label: 'PDF (klassisch)' },
  { value: 'zugferd', label: 'ZUGFeRD (PDF mit XML)' },
  { value: 'xrechnung', label: 'XRechnung (XML, öffentliche Auftraggeber)' },
];

export const SALUTATIONS = [
  { value: '', label: '–' },
  { value: 'Herr', label: 'Herr' },
  { value: 'Frau', label: 'Frau' },
  { value: 'Divers', label: 'Divers' },
];

export const COUNTRIES: { value: string; label: string }[] = [
  ['DE', 'Deutschland'], ['AT', 'Österreich'], ['CH', 'Schweiz'], ['BE', 'Belgien'], ['BG', 'Bulgarien'], ['CY', 'Zypern'],
  ['CZ', 'Tschechien'], ['DK', 'Dänemark'], ['EE', 'Estland'], ['ES', 'Spanien'], ['FI', 'Finnland'], ['FR', 'Frankreich'],
  ['GR', 'Griechenland'], ['HR', 'Kroatien'], ['HU', 'Ungarn'], ['IE', 'Irland'], ['IT', 'Italien'], ['LT', 'Litauen'],
  ['LU', 'Luxemburg'], ['LV', 'Lettland'], ['MT', 'Malta'], ['NL', 'Niederlande'], ['PL', 'Polen'], ['PT', 'Portugal'],
  ['RO', 'Rumänien'], ['SE', 'Schweden'], ['SI', 'Slowenien'], ['SK', 'Slowakei'], ['GB', 'Vereinigtes Königreich'],
  ['US', 'Vereinigte Staaten'], ['NO', 'Norwegen'], ['LI', 'Liechtenstein'], ['CA', 'Kanada'], ['AU', 'Australien'],
  ['JP', 'Japan'], ['CN', 'China'], ['TR', 'Türkei'],
].map(([value, label]) => ({ value, label: `${label}` }));

export const countryName = (code: string) => COUNTRIES.find((c) => c.value === code)?.label ?? code;

export const ACCOUNT_KINDS: Record<string, string> = {
  asset: 'Anlagevermögen',
  current: 'Umlaufvermögen',
  liability: 'Verbindlichkeiten',
  equity: 'Eigenkapital',
  private: 'Privat',
  tax: 'Steuern',
  revenue: 'Erlöse',
  expense: 'Aufwand',
  neutral: 'Sonstige',
};

export const SOURCE_LABELS: Record<string, string> = {
  document: 'Rechnung',
  receipt: 'Beleg',
  transaction: 'Zahlung',
  writeoff: 'Ausbuchung',
  asset: 'Abschreibung',
  manual: 'Manuell',
  opening: 'Eröffnung',
};

export const ASSET_METHODS = [
  { value: 'linear', label: 'Linear (pro rata temporis)' },
  { value: 'gwg', label: 'GWG – Sofortabschreibung (bis 800 €)' },
  { value: 'pool', label: 'Sammelposten (250–1.000 €, 5 Jahre)' },
  { value: 'none', label: 'Keine Abschreibung' },
];

export const BOOKKEEPING = [
  { value: 'euer', label: 'Einnahmen-Überschuss-Rechnung (EÜR)', description: 'Einfache Buchführung für Freiberufler, Kleingewerbe und Einzelunternehmen. Einnahmen und Ausgaben werden bei Zahlung erfasst (Ist-Versteuerung).' },
  { value: 'bilanz', label: 'Doppelte Buchführung (Bilanzierung)', description: 'Für Kapitalgesellschaften und Kaufleute. Forderungen und Verbindlichkeiten werden zum Rechnungsdatum gebucht (Soll-Versteuerung), Jahresabschluss mit Bilanz und GuV.' },
];
