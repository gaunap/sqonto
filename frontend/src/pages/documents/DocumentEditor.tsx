// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useRef, useState } from 'react';
import { useBlocker, useNavigate, useParams, useSearchParams } from 'react-router';
import {
  Alert,
  Box,
  Button,
  Card,
  Center,
  Collapse,
  Drawer,
  Grid,
  Group,
  Loader,
  Menu,
  Modal,
  NumberInput,
  SegmentedControl,
  Select,
  SimpleGrid,
  Stack,
  Switch,
  Text,
  TextInput,
  Textarea,
  Tooltip,
} from '@mantine/core';
import { useDebouncedValue, useHotkeys, useMediaQuery } from '@mantine/hooks';
import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { IconBraces, IconChevronDown, IconChevronUp, IconDeviceFloppy, IconEye, IconEyeOff, IconLock, IconSend, IconInfoCircle } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import { notifyError, useAction, useApi, useLayouts, useSettings } from '@/api/hooks';
import type { ContactDefaults, Document, DocumentFull, DocumentInput, MandateView, PreviewResult } from '@/api/types';
import { ContactSelect, DateField } from '@/components/inputs';
import { emptyLine, LinesEditor, newKey, type EditorLine } from '@/components/LinesEditor';
import { PagesPreview, TotalsBox } from '@/components/DocumentParts';
import { Page, PageHeader, StatusBadge } from '@/components/ui';
import { calculate } from '@/lib/calc';
import { date as fmtDate, formatIban, money, today } from '@/lib/format';
import { useLeaveWarning } from '@/lib/desktop';
import { DOC_TYPES, EINVOICE_FORMATS, TAX_MODES, docLabel, type DocType } from '@/lib/labels';

const PLACEHOLDERS = ['Anrede', 'Kunde', 'Ansprechpartner', 'Nummer', 'Datum', 'Betrag', 'Fällig am', 'Zahlungsziel', 'Gültig bis', 'Skonto', 'Firma', 'Referenz', 'Kundennummer', 'Leistungszeitraum', 'Monat', 'Quartal', 'Jahr', 'Mandatsreferenz', 'Gläubiger-ID', 'Kunden-IBAN'];

function PlaceholderMenu({ onInsert }: { onInsert: (p: string) => void }) {
  return (
    <Menu position="bottom-end" width={220}>
      <Menu.Target>
        <Button size="compact-xs" variant="subtle" leftSection={<IconBraces size={12} />}>
          Platzhalter
        </Button>
      </Menu.Target>
      <Menu.Dropdown mah={300} style={{ overflowY: 'auto' }}>
        <Menu.Label>Wird beim Erstellen ersetzt</Menu.Label>
        {PLACEHOLDERS.map((p) => (
          <Menu.Item key={p} onClick={() => onInsert(`{{${p}}}`)}>
            {`{{${p}}}`}
          </Menu.Item>
        ))}
      </Menu.Dropdown>
    </Menu>
  );
}

function toEditorLines(lines: DocumentInput['lines']): EditorLine[] {
  return lines.map((l) => ({ ...l, _key: newKey() }));
}

export function DocumentEditorPage() {
  const { id } = useParams();
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const isNew = !id;
  const typeParam = (params.get('type') ?? 'invoice') as DocType;
  const contactParam = params.get('contact');
  const { data: settings } = useSettings();
  const { data: layouts } = useLayouts();

  const [doc, setDoc] = useState<Document | null>(null);
  const [lines, setLines] = useState<EditorLine[]>([]);
  const [dirty, setDirty] = useState(false);
  const wide = useMediaQuery('(min-width: 1680px)', window.innerWidth >= 1680);
  const [showPreview, setShowPreview] = useState(() => window.innerWidth >= 1680);
  const sidePreview = showPreview && wide;
  const [moreOpen, setMoreOpen] = useState(false);
  const [persons, setPersons] = useState<string[]>([]);
  const [useRange, setUseRange] = useState(false);
  const { data: mandates } = useApi<MandateView[]>(doc?.contact_id && doc.doc_type === 'invoice' ? '/mandates' : null, { contact_id: doc?.contact_id ?? undefined });
  const loadedFor = useRef<string>('');

  // Load template or existing document
  useEffect(() => {
    const key = isNew ? `new-${typeParam}-${contactParam}` : `edit-${id}`;
    if (loadedFor.current === key) return;
    loadedFor.current = key;
    (async () => {
      try {
        if (isNew) {
          const tpl = await api.get<DocumentInput>('/documents/new', { doc_type: typeParam, contact_id: contactParam ?? undefined });
          setDoc(tpl.doc);
          setLines(tpl.lines.length ? toEditorLines(tpl.lines) : [emptyLine({ tax_rate: tpl.doc.tax_mode === 'standard' ? 19 : 0 })]);
          setUseRange(false);
          if (tpl.doc.contact_id) {
            const d = await api.get<ContactDefaults>(`/contacts/${tpl.doc.contact_id}/defaults`, { doc_type: typeParam });
            setPersons(d.persons);
          }
        } else {
          const full = await api.get<DocumentFull>(`/documents/${id}`);
          if (full.doc.status !== 'draft' && (full.doc.doc_type === 'invoice' || full.doc.doc_type === 'credit_note')) {
            navigate(`/dokumente/${id}`, { replace: true });
            return;
          }
          setDoc(full.doc);
          setLines(toEditorLines(full.lines));
          setUseRange(!!full.doc.service_date_end);
          if (full.doc.contact_id) {
            const d = await api.get<ContactDefaults>(`/contacts/${full.doc.contact_id}/defaults`, { doc_type: full.doc.doc_type });
            setPersons(d.persons);
          }
        }
        setDirty(false);
      } catch (e) {
        notifyError(e, 'Dokument konnte nicht geladen werden');
      }
    })();
  }, [id, isNew, typeParam, contactParam, navigate]);

  const blocker = useBlocker(({ currentLocation, nextLocation }) => dirty && currentLocation.pathname !== nextLocation.pathname);
  useLeaveWarning(dirty);

  const patch = (p: Partial<Document>) => {
    setDoc((d) => (d ? { ...d, ...p } : d));
    setDirty(true);
  };
  const changeLines = (l: EditorLine[]) => {
    setLines(l);
    setDirty(true);
  };

  const payload = useMemo<DocumentInput | null>(() => (doc ? { doc, lines: lines.map(({ _key, ...l }) => l) } : null), [doc, lines]);
  const totals = useMemo(() => (doc ? calculate(doc, lines).totals : null), [doc, lines]);

  const [debouncedPayload] = useDebouncedValue(payload, 650);
  const preview = useQuery({
    queryKey: ['doc-preview', debouncedPayload ? JSON.stringify(debouncedPayload) : ''],
    queryFn: () => api.post<PreviewResult>('/documents/preview', debouncedPayload),
    enabled: showPreview && !!debouncedPayload,
    placeholderData: keepPreviousData,
    staleTime: 60_000,
  });

  const [finalizeOpen, setFinalizeOpen] = useState(false);
  const save = useAction(
    async (opts: { finalize?: boolean; redate?: string }) => {
      if (!payload) throw new Error('Kein Dokument');
      // "Auf heute setzen": the new date goes along with the save before finalizing.
      const body = opts.redate ? { ...payload, doc: { ...payload.doc, ...redated(payload.doc, opts.redate) } } : payload;
      const saved = isNew || !doc?.id ? await api.post<DocumentFull>('/documents', body) : await api.put<DocumentFull>(`/documents/${doc.id}`, body);
      if (opts.finalize) {
        return api.post<DocumentFull>(`/documents/${saved.doc.id}/finalize`);
      }
      return saved;
    },
    {
      success: (r, opts) => (opts.finalize ? `${docLabel(r.doc.doc_type, r.doc.kind)} ${r.doc.number} wurde abgeschlossen` : 'Entwurf gespeichert'),
      onSuccess: (r, opts) => {
        setDirty(false);
        if (opts.finalize) {
          setTimeout(() => navigate(`/dokumente/${r.doc.id}`), 0);
        } else if (isNew) {
          setTimeout(() => navigate(`/dokumente/${r.doc.id}/bearbeiten`, { replace: true }), 0);
          loadedFor.current = `edit-${r.doc.id}`;
          setDoc(r.doc);
          setLines(toEditorLines(r.lines));
        } else {
          setDoc(r.doc);
        }
      },
    },
  );

  useHotkeys([['mod+S', () => save.mutate({})]], []);

  if (!doc || !settings) {
    return (
      <Center h="60vh">
        <Loader />
      </Center>
    );
  }

  const meta = DOC_TYPES[doc.doc_type as DocType];
  const isInvoiceLike = doc.doc_type === 'invoice' || doc.doc_type === 'credit_note';
  const label = docLabel(doc.doc_type, doc.kind);
  const smallBusiness = settings.tax.small_business;

  /** Payment method with the matching default closing text (transfer request or pre-notification). */
  const paymentPatch = (method: string, mandateId: number | null): Partial<Document> => {
    if (doc.doc_type !== 'invoice' || doc.kind === 'cancellation') return {};
    const transfer = settings.docs.texts.invoice?.closing ?? '';
    const debit = settings.docs.texts.invoice_debit?.closing ?? '';
    let closing = doc.closing_text;
    if (doc.kind === 'standard' && method === 'debit' && closing === transfer) closing = debit;
    if (doc.kind === 'standard' && method !== 'debit' && closing === debit) closing = transfer;
    return { payment_method: method, mandate_id: method === 'debit' ? mandateId : null, closing_text: closing };
  };
  const debit = doc.payment_method === 'debit';
  const prenotification = settings.debit.prenotification_days;

  const onContact = async (contactId: number | null) => {
    if (!contactId) {
      patch({ contact_id: null, ...paymentPatch('', null) });
      setPersons([]);
      return;
    }
    try {
      const d = await api.get<ContactDefaults>(`/contacts/${contactId}/defaults`, { doc_type: doc.doc_type });
      const days = d.payment_terms_days ?? doc.payment_terms_days;
      patch({
        contact_id: contactId,
        address: d.address,
        contact_person: d.contact_person,
        tax_mode: smallBusiness ? 'small_business' : d.tax_mode,
        buyer_reference: d.buyer_reference,
        einvoice_format: d.einvoice_format,
        payment_terms_days: isInvoiceLike ? days : doc.payment_terms_days,
        due_date: isInvoiceLike && days != null ? dayjs(doc.date).add(days, 'day').format('YYYY-MM-DD') : doc.due_date,
        ...paymentPatch(d.payment_method, d.mandate_id),
      });
      setPersons(d.persons);
    } catch (e) {
      notifyError(e);
    }
  };

  const setDate = (v: string | null) => {
    if (!v) return;
    patch(redated(doc, v));
  };

  const finalize = () => {
    if (lines.filter((l) => l.line_type === 'item').length === 0) {
      notifyError(new Error('Bitte mindestens eine Position erfassen.'));
      return;
    }
    setFinalizeOpen(true);
  };
  const daysOld = dayjs(today()).diff(dayjs(doc.date), 'day');
  const confirmFinalize = (redate?: string) => {
    setFinalizeOpen(false);
    save.mutate({ finalize: true, redate });
  };

  const insertInto = (field: 'intro_text' | 'closing_text', text: string) => patch({ [field]: (doc[field] ? doc[field] + ' ' : '') + text } as Partial<Document>);

  return (
    <Page size="full">
      <PageHeader
        back={{ to: meta?.route ?? '/', label: meta?.plural ?? 'Zurück' }}
        title={doc.number ? `${label} ${doc.number}` : isNew ? (doc.kind === 'standard' ? meta?.newLabel : `Neue ${label}`) : `${label} (Entwurf)`}
        badge={<StatusBadge status={doc.status} docType={doc.doc_type} />}
        actions={
          <>
            <Tooltip label={showPreview ? 'Vorschau ausblenden' : 'Live-Vorschau anzeigen'}>
              <Button variant="default" leftSection={showPreview ? <IconEyeOff size={16} /> : <IconEye size={16} />} onClick={() => setShowPreview((s) => !s)}>
                Vorschau
              </Button>
            </Tooltip>
            <Button variant="default" leftSection={<IconDeviceFloppy size={16} />} loading={save.isPending && !save.variables?.finalize} onClick={() => save.mutate({})}>
              {doc.status === 'draft' ? 'Entwurf speichern' : 'Speichern'}
            </Button>
            {doc.status === 'draft' && (
              <Button leftSection={isInvoiceLike ? <IconLock size={16} /> : <IconSend size={16} />} loading={save.isPending && !!save.variables?.finalize} onClick={finalize}>
                Abschließen
              </Button>
            )}
          </>
        }
      />

      <Grid gap="lg">
        <Grid.Col span={sidePreview ? 7 : 12}>
          <Stack gap="lg">
            <Card>
              <Grid gap="lg">
                <Grid.Col span={{ base: 12, md: 6 }}>
                  <Stack gap="xs">
                    <ContactSelect value={doc.contact_id} onChange={(cid) => onContact(cid)} role="customer" label="Empfänger" />
                    {persons.length > 0 && (
                      <Select
                        label="Ansprechpartner"
                        data={persons}
                        value={doc.contact_person || null}
                        clearable
                        onChange={(p) => patch({ contact_person: p ?? '' })}
                        allowDeselect
                      />
                    )}
                    <Textarea label="Anschrift" description="Wird aus dem Kontakt übernommen und kann angepasst werden" minRows={4} value={doc.address} onChange={(e) => patch({ address: e.currentTarget.value })} />
                  </Stack>
                </Grid.Col>
                <Grid.Col span={{ base: 12, md: 6 }}>
                  <SimpleGrid cols={2} spacing="sm" verticalSpacing="xs">
                    <TextInput label={meta?.numberLabel ?? 'Nummer'} value={doc.number} placeholder="automatisch" readOnly />
                    <DateField label="Datum" value={doc.date} onChange={setDate} />
                    {doc.doc_type !== 'quote' && (
                      <DateField label={useRange ? 'Leistung von' : 'Leistungsdatum'} value={doc.service_date} onChange={(v) => patch({ service_date: v })} clearable />
                    )}
                    {doc.doc_type !== 'quote' && useRange && <DateField label="Leistung bis" value={doc.service_date_end} onChange={(v) => patch({ service_date_end: v })} clearable />}
                    {doc.doc_type === 'quote' && <DateField label="Gültig bis" value={doc.valid_until} onChange={(v) => patch({ valid_until: v })} clearable />}
                    {isInvoiceLike && doc.kind !== 'cancellation' && (
                      <>
                        <NumberInput
                          label="Zahlungsziel"
                          suffix=" Tage"
                          min={0}
                          max={365}
                          value={doc.payment_terms_days ?? ''}
                          onChange={(v) => {
                            const days = v === '' ? null : Number(v);
                            patch({ payment_terms_days: days, due_date: days != null ? dayjs(doc.date).add(days, 'day').format('YYYY-MM-DD') : doc.due_date });
                          }}
                        />
                        <DateField label={debit ? 'Einzug am' : 'Fällig am'} value={doc.due_date} onChange={(v) => patch({ due_date: v, payment_terms_days: v ? dayjs(v).diff(dayjs(doc.date), 'day') : doc.payment_terms_days })} />
                      </>
                    )}
                    {doc.doc_type === 'invoice' && doc.kind !== 'cancellation' && (
                      <>
                        <Select
                          label="Zahlungsart"
                          allowDeselect={false}
                          value={debit ? 'debit' : 'transfer'}
                          data={[
                            { value: 'transfer', label: 'Überweisung' },
                            { value: 'debit', label: 'SEPA-Lastschrift', disabled: !(mandates ?? []).some((m) => m.usable) && !debit },
                          ]}
                          onChange={(v) => {
                            const m = (mandates ?? []).find((x) => x.usable);
                            patch(paymentPatch(v === 'debit' ? 'debit' : '', doc.mandate_id ?? m?.mandate.id ?? null));
                          }}
                          description={!debit && doc.contact_id && !(mandates ?? []).some((m) => m.usable) ? 'Lastschrift: zuerst ein Mandat beim Kontakt erfassen' : undefined}
                          error={debit && (doc.payment_terms_days ?? 0) < prenotification ? `Vorabankündigung: Einzug frühestens ${prenotification} Tage nach Rechnungsdatum` : undefined}
                        />
                        {debit && (
                          <Select
                            label="Mandat"
                            allowDeselect={false}
                            value={doc.mandate_id ? String(doc.mandate_id) : null}
                            data={(mandates ?? [])
                              .filter((m) => m.usable || m.mandate.id === doc.mandate_id)
                              .map((m) => ({ value: String(m.mandate.id), label: `${m.mandate.reference} · ${formatIban(m.mandate.iban)}${m.usable ? '' : ` (${m.status_label})`}` }))}
                            onChange={(v) => patch({ mandate_id: v ? Number(v) : null })}
                            error={!doc.mandate_id ? 'Bitte ein Mandat wählen' : (mandates ?? []).find((m) => m.mandate.id === doc.mandate_id && !m.usable)?.status_label}
                          />
                        )}
                      </>
                    )}
                    <TextInput label="Ihre Referenz / Bestellnr." value={doc.reference} onChange={(e) => patch({ reference: e.currentTarget.value })} />
                    {(doc.einvoice_format === 'xrechnung' || doc.buyer_reference) && (
                      <TextInput label="Leitweg-ID" value={doc.buyer_reference} onChange={(e) => patch({ buyer_reference: e.currentTarget.value })} />
                    )}
                  </SimpleGrid>
                  {doc.doc_type !== 'quote' && (
                    <Switch
                      mt="sm"
                      size="xs"
                      label="Leistungszeitraum statt Leistungsdatum"
                      checked={useRange}
                      onChange={(e) => {
                        setUseRange(e.currentTarget.checked);
                        if (!e.currentTarget.checked) patch({ service_date_end: null });
                      }}
                    />
                  )}
                </Grid.Col>
              </Grid>
            </Card>

            <Card>
              <Stack gap="md">
                <TextInput label="Betreff / Titel" value={doc.title} onChange={(e) => patch({ title: e.currentTarget.value })} styles={{ input: { fontWeight: 600 } }} />
                <Box>
                  <Group justify="space-between" mb={4}>
                    <Text size="sm" fw={500}>
                      Einleitungstext
                    </Text>
                    <PlaceholderMenu onInsert={(p) => insertInto('intro_text', p)} />
                  </Group>
                  <Textarea minRows={3} value={doc.intro_text} onChange={(e) => patch({ intro_text: e.currentTarget.value })} />
                </Box>
                {doc.doc_type === 'delivery' && (
                  <Alert icon={<IconInfoCircle size={16} />} color="gray" variant="light" py="xs">
                    Auf dem Lieferschein werden keine Preise gedruckt. Die Preise werden bei der Umwandlung in eine Rechnung übernommen.
                  </Alert>
                )}
                <LinesEditor doc={doc} lines={lines} onChange={changeLines} />
                {totals && doc.doc_type !== 'delivery' && <TotalsBox totals={totals} doc={doc} />}
                <Box>
                  <Group justify="space-between" mb={4}>
                    <Text size="sm" fw={500}>
                      Schlusstext / Zahlungsbedingungen
                    </Text>
                    <PlaceholderMenu onInsert={(p) => insertInto('closing_text', p)} />
                  </Group>
                  <Textarea minRows={3} value={doc.closing_text} onChange={(e) => patch({ closing_text: e.currentTarget.value })} />
                </Box>
              </Stack>
            </Card>

            <Card>
              <Group justify="space-between" onClick={() => setMoreOpen((o) => !o)} style={{ cursor: 'pointer' }}>
                <Box>
                  <Text fw={650}>Steuer, Layout & weitere Optionen</Text>
                  <Text size="xs" c="dimmed">
                    {TAX_MODES.find((t) => t.value === doc.tax_mode)?.label} · {doc.prices_gross ? 'Bruttopreise' : 'Nettopreise'} ·{' '}
                    {layouts?.find((l) => l.id === doc.layout_id)?.name ?? 'Standard-Layout'} · {EINVOICE_FORMATS.find((f) => f.value === doc.einvoice_format)?.label ?? 'PDF'}
                  </Text>
                </Box>
                {moreOpen ? <IconChevronUp size={18} /> : <IconChevronDown size={18} />}
              </Group>
              <Collapse expanded={moreOpen}>
                <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm" mt="md">
                  <Select
                    label="Steuerart"
                    data={TAX_MODES}
                    value={doc.tax_mode}
                    disabled={smallBusiness}
                    inputWrapperOrder={['label', 'input', 'description', 'error']}
                    description={smallBusiness ? 'Kleinunternehmerregelung ist in den Einstellungen aktiv' : undefined}
                    onChange={(v) => patch({ tax_mode: v ?? 'standard' })}
                  />
                  <Box>
                    <Text size="sm" fw={500} mb={4}>
                      Preisangabe
                    </Text>
                    <SegmentedControl
                      fullWidth
                      value={doc.prices_gross ? 'gross' : 'net'}
                      onChange={(v) => patch({ prices_gross: v === 'gross' })}
                      data={[
                        { value: 'net', label: 'Netto (B2B)' },
                        { value: 'gross', label: 'Brutto (B2C)' },
                      ]}
                    />
                  </Box>
                  <Select
                    label="Layout"
                    data={(layouts ?? []).map((l) => ({ value: String(l.id), label: l.name + (l.is_default ? ' (Standard)' : '') }))}
                    value={doc.layout_id ? String(doc.layout_id) : null}
                    placeholder="Standard-Layout"
                    clearable
                    allowDeselect
                    onChange={(v) => patch({ layout_id: v ? Number(v) : null })}
                  />
                  {isInvoiceLike && <Select label="Rechnungsformat" data={EINVOICE_FORMATS} value={doc.einvoice_format || 'pdf'} onChange={(v) => patch({ einvoice_format: v ?? 'pdf' })} />}
                  {doc.doc_type === 'invoice' && doc.kind !== 'cancellation' && (
                    <>
                      <NumberInput label="Skonto" suffix=" %" min={0} max={20} decimalScale={2} decimalSeparator="," value={doc.cash_discount_percent || ''} onChange={(v) => patch({ cash_discount_percent: Number(v) || 0 })} />
                      <NumberInput label="Skontofrist" suffix=" Tage" min={0} max={90} value={doc.cash_discount_days || ''} onChange={(v) => patch({ cash_discount_days: Number(v) || 0 })} />
                    </>
                  )}
                </SimpleGrid>
                <Textarea mt="sm" label="Interne Notiz" description="Erscheint nicht auf dem Dokument" value={doc.notes} onChange={(e) => patch({ notes: e.currentTarget.value })} />
              </Collapse>
            </Card>
          </Stack>
        </Grid.Col>
        {sidePreview && (
          <Grid.Col span={5}>
            <Box pos="sticky" top="var(--app-sticky-top)">
              <Group justify="space-between" mb="xs">
                <Text size="sm" fw={600} c="dimmed">
                  Live-Vorschau
                </Text>
                {preview.isFetching && <Loader size="xs" />}
              </Group>
              <Box mah="calc(100vh - 120px)" style={{ overflowY: 'auto' }} pb="md" px={4}>
                <PagesPreview pages={preview.data?.pages} error={preview.error ? (preview.error as Error).message : null} />
              </Box>
            </Box>
          </Grid.Col>
        )}
      </Grid>

      <Drawer opened={showPreview && !wide} onClose={() => setShowPreview(false)} size="50%" title={<Group gap="xs"><Text fw={650}>Live-Vorschau</Text>{preview.isFetching && <Loader size="xs" />}</Group>} styles={{ body: { background: 'var(--app-bg)', minHeight: '100%' } }}>
        <PagesPreview pages={preview.data?.pages} error={preview.error ? (preview.error as Error).message : null} />
      </Drawer>

      <Modal opened={blocker.state === 'blocked'} onClose={() => blocker.reset?.()} title="Ungespeicherte Änderungen">
        <Text size="sm">Sie haben Änderungen, die noch nicht gespeichert wurden. Möchten Sie die Seite trotzdem verlassen?</Text>
        <Group justify="flex-end" mt="lg">
          <Button variant="default" onClick={() => blocker.reset?.()}>
            Weiter bearbeiten
          </Button>
          <Button color="red" variant="light" onClick={() => blocker.proceed?.()}>
            Verwerfen
          </Button>
          <Button
            onClick={() => {
              blocker.reset?.();
              save.mutate({});
            }}
          >
            Speichern
          </Button>
        </Group>
      </Modal>
      <Modal opened={finalizeOpen} onClose={() => setFinalizeOpen(false)} title={<Text fw={650}>{label} abschließen</Text>}>
        <Stack gap="sm">
          <Text size="sm">
            {isInvoiceLike
              ? `Die ${label} erhält eine fortlaufende Nummer und wird gebucht. Nach dem Abschließen kann sie nicht mehr geändert, sondern nur noch storniert werden (GoBD).`
              : 'Das Dokument erhält eine fortlaufende Nummer und kann anschließend als PDF versendet werden.'}
          </Text>
          <Text size="sm">
            Datum <b>{fmtDate(doc.date)}</b>
            {isInvoiceLike && doc.due_date ? ` · fällig ${fmtDate(doc.due_date)}` : ''}
            {isInvoiceLike ? ` · Gesamtbetrag ${money(totals?.gross ?? 0)}` : ''}
          </Text>
          {daysOld > 0 && (
            <Alert color="yellow" icon={<IconInfoCircle size={16} />} p="sm">
              <Text size="sm">
                Das Datum liegt {daysOld === 1 ? 'einen Tag' : `${daysOld} Tage`} zurück. Soll das Dokument mit dem heutigen Datum abgeschlossen werden?
              </Text>
            </Alert>
          )}
          <Group justify="flex-end">
            <Button variant="default" onClick={() => setFinalizeOpen(false)}>
              Abbrechen
            </Button>
            {daysOld > 0 ? (
              <>
                <Button variant="default" onClick={() => confirmFinalize()}>
                  Mit {fmtDate(doc.date)} abschließen
                </Button>
                <Button onClick={() => confirmFinalize(today())}>Auf heute setzen und abschließen</Button>
              </>
            ) : (
              <Button onClick={() => confirmFinalize()}>Abschließen</Button>
            )}
          </Group>
        </Stack>
      </Modal>
    </Page>
  );
}

/** Moves a document to another date: the due date follows the payment terms, a single service date
 *  that equals the old date moves along. */
function redated(d: Document, v: string): Partial<Document> {
  const p: Partial<Document> = { date: v };
  if ((d.doc_type === 'invoice' || d.doc_type === 'credit_note') && d.payment_terms_days != null) p.due_date = dayjs(v).add(d.payment_terms_days, 'day').format('YYYY-MM-DD');
  if (!d.service_date_end && d.service_date === d.date) p.service_date = v;
  return p;
}
