// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router';
import {
  ActionIcon,
  Alert,
  Badge,
  Box,
  Button,
  Card,
  Center,
  Divider,
  Grid,
  Group,
  List,
  Loader,
  NumberInput,
  SegmentedControl,
  Select,
  SimpleGrid,
  Stack,
  Switch,
  Table,
  Text,
  TextInput,
  Textarea,
  ThemeIcon,
  Tooltip,
} from '@mantine/core';
import { Dropzone } from '@mantine/dropzone';
import { useHotkeys } from '@mantine/hooks';
import {
  IconAlertTriangle,
  IconCash,
  IconCloudUpload,
  IconDeviceFloppy,
  IconExternalLink,
  IconFileCode,
  IconPlus,
  IconTrash,
  IconBuildingWarehouse,
  IconRefresh,
  IconUserPlus,
} from '@tabler/icons-react';
import { api, apiUrl } from '@/api/client';
import { notifyError, useAction, useApi, useChart, usePaymentAccounts, useSettings } from '@/api/hooks';
import { emptyContact } from '@/components/ContactDrawer';
import type { AccountSuggestion, Contact, FileMeta, ImportResult, ParsedInvoice, Receipt, ReceiptFull, ReceiptInput, ReceiptLine } from '@/api/types';
import { AccountSelect, ContactSelect, DateField, MoneyInput } from '@/components/inputs';
import { PaymentModal } from '@/components/PaymentModal';
import { Page, PageHeader, StatusBadge, UnsavedChangesGuard, confirmAction } from '@/components/ui';
import { receiptIncoming, receiptRemaining } from '@/lib/calc';
import { date, money, r2, today } from '@/lib/format';
import { isDesktop } from '@/lib/desktop';
import { RECEIPT_TAX_MODES, TAX_RATES } from '@/lib/labels';
import { RECEIPT_ACCEPT } from './ReceiptsPage';

function emptyReceipt(): Receipt {
  return {
    id: 0,
    number: '',
    receipt_type: 'expense',
    contact_id: null,
    supplier_name: '',
    invoice_number: '',
    date: today(),
    due_date: null,
    service_date: null,
    description: '',
    tax_mode: 'standard',
    prices_gross: true,
    net_total: 0,
    tax_total: 0,
    gross_total: 0,
    paid_total: 0,
    status: 'open',
    file_id: null,
    source: 'manual',
    iban: '',
    notes: '',
    created_at: '',
    updated_at: '',
  };
}

function emptyRLine(account = '', rate = 19): ReceiptLine {
  return { id: 0, receipt_id: 0, position: 0, account, description: '', net: 0, tax_rate: rate, tax: 0, gross: 0, asset_id: null, asset_life_years: 0 };
}

function computeLine(r: Receipt, l: ReceiptLine): ReceiptLine {
  if (r.tax_mode !== 'standard') {
    const net = r2(r.prices_gross ? l.gross : l.net);
    return { ...l, net, tax: 0, gross: net };
  }
  if (r.prices_gross) {
    const net = r2(l.gross / (1 + l.tax_rate / 100));
    return { ...l, net, tax: r2(l.gross - net) };
  }
  const tax = r2((l.net * l.tax_rate) / 100);
  return { ...l, tax, gross: r2(l.net + tax) };
}

function FilePanel({ file, onUpload, uploading, parsed }: { file: FileMeta | null; onUpload: (f: File) => void; uploading: boolean; parsed: ParsedInvoice | null }) {
  const url = file ? apiUrl(`/files/${file.id}`) : '';
  const isPdf = file?.mime === 'application/pdf';
  const isImage = file?.mime.startsWith('image/');
  const isXml = file?.mime.includes('xml');
  return (
    <Card padding="md" pos="sticky" top="var(--app-sticky-top)">
      {!file ? (
        <Dropzone onDrop={(f) => f[0] && onUpload(f[0])} accept={RECEIPT_ACCEPT} maxFiles={1} loading={uploading} h={520} radius="md">
          <Center h={480} style={{ pointerEvents: 'none' }}>
            <Stack align="center" gap="xs">
              <ThemeIcon size={56} radius="xl" variant="light">
                <IconCloudUpload size={30} />
              </ThemeIcon>
              <Text fw={600}>Belegdatei hinzufügen</Text>
              <Text size="sm" c="dimmed" ta="center" maw={280}>
                PDF, Bild oder E-Rechnung (XRechnung, ZUGFeRD) hierher ziehen – E-Rechnungen werden automatisch ausgelesen.
              </Text>
            </Stack>
          </Center>
        </Dropzone>
      ) : (
        <Stack gap="sm" h="100%">
          <Group justify="space-between" wrap="nowrap">
            <Text size="sm" fw={600} truncate>
              {file.filename}
            </Text>
            <Group gap={4} wrap="nowrap">
              <Tooltip label={isDesktop ? 'In eigenem Fenster öffnen' : 'In neuem Tab öffnen'}>
                <ActionIcon component="a" href={url} target="_blank" aria-label="Datei öffnen">
                  <IconExternalLink size={16} />
                </ActionIcon>
              </Tooltip>
            </Group>
          </Group>
          {isPdf && <iframe title="Beleg" src={`${url}#toolbar=0&view=FitH`} style={{ width: '100%', height: 640, border: '1px solid var(--app-border)', borderRadius: 8, background: '#fff' }} />}
          {isImage && <img src={url} alt="Beleg" style={{ width: '100%', borderRadius: 8, border: '1px solid var(--app-border)' }} />}
          {isXml && !parsed && (
            <Alert icon={<IconFileCode size={16} />} color="violet">
              XML-Datei
            </Alert>
          )}
          {parsed && parsed.number && <ParsedView parsed={parsed} />}
        </Stack>
      )}
    </Card>
  );
}

function ParsedView({ parsed }: { parsed: ParsedInvoice }) {
  return (
    <Card withBorder radius="md" padding="md" bg="var(--mantine-color-violet-0)" style={{ borderColor: 'var(--mantine-color-violet-2)' }}>
      <Group gap="xs" mb="xs">
        <ThemeIcon color="violet" variant="light" size="sm">
          <IconFileCode size={14} />
        </ThemeIcon>
        <Text fw={650} size="sm">
          E-Rechnung erkannt ({parsed.syntax}
          {parsed.profile.includes('xrechnung') ? ', XRechnung' : parsed.profile ? ', EN 16931' : ''})
        </Text>
      </Group>
      <SimpleGrid cols={2} spacing={4} verticalSpacing={2}>
        <Text size="xs" c="dimmed">Rechnungsnr.</Text>
        <Text size="xs">{parsed.number}</Text>
        <Text size="xs" c="dimmed">Rechnungssteller</Text>
        <Text size="xs">{parsed.seller.name}</Text>
        {parsed.seller.vat_id && (
          <>
            <Text size="xs" c="dimmed">USt-IdNr.</Text>
            <Text size="xs">{parsed.seller.vat_id}</Text>
          </>
        )}
        <Text size="xs" c="dimmed">Datum / fällig</Text>
        <Text size="xs">
          {date(parsed.issue_date)} / {date(parsed.due_date)}
        </Text>
        {parsed.iban && (
          <>
            <Text size="xs" c="dimmed">IBAN</Text>
            <Text size="xs">{parsed.iban}</Text>
          </>
        )}
      </SimpleGrid>
      <Table mt="sm" fz="xs" verticalSpacing={2} withRowBorders={false}>
        <Table.Tbody>
          {parsed.lines.map((l, i) => (
            <Table.Tr key={i}>
              <Table.Td>
                {l.quantity} × {l.name}
              </Table.Td>
              <Table.Td ta="right">{l.tax_rate} %</Table.Td>
              <Table.Td ta="right" className="tabular">
                {money(l.net)}
              </Table.Td>
            </Table.Tr>
          ))}
          <Table.Tr>
            <Table.Td fw={700}>Gesamt brutto</Table.Td>
            <Table.Td />
            <Table.Td ta="right" fw={700} className="tabular">
              {money(parsed.gross_total)}
            </Table.Td>
          </Table.Tr>
        </Table.Tbody>
      </Table>
    </Card>
  );
}

export function ReceiptEditorPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const importResult = (location.state as { import?: ImportResult } | null)?.import ?? null;
  const isNew = !id;
  const { data: settings } = useSettings();
  const { data: chart } = useChart();
  const { data: accounts } = usePaymentAccounts();
  const { data: full, refetch } = useApi<ReceiptFull>(isNew ? null : `/receipts/${id}`);

  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [lines, setLines] = useState<ReceiptLine[]>([]);
  const [file, setFile] = useState<FileMeta | null>(null);
  const [parsed, setParsed] = useState<ParsedInvoice | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [paidNow, setPaidNow] = useState(false);
  const [payAccount, setPayAccount] = useState<string | null>(null);
  const [payDate, setPayDate] = useState<string | null>(today());
  const [uploading, setUploading] = useState(false);
  const [payOpen, setPayOpen] = useState(false);
  // Unsaved changes: compare with the state that was loaded or last saved ('' = never saved import).
  const [baseline, setBaseline] = useState<string | null>(null);
  const snapshotOf = (r: Receipt | null, l: ReceiptLine[], f: FileMeta | null) => JSON.stringify({ r, l, f: f?.id ?? null });
  const dirty = baseline !== null && !!receipt && snapshotOf(receipt, lines, file) !== baseline;

  useEffect(() => {
    if (isNew) {
      if (importResult) {
        setReceipt(importResult.draft.receipt);
        setLines(importResult.draft.lines.length ? importResult.draft.lines : [emptyRLine()]);
        setParsed(importResult.parsed.number ? importResult.parsed : null);
        setWarnings(importResult.warnings);
        setFile(importResult.file);
        setBaseline('');
      } else {
        const r = emptyReceipt();
        const l = [emptyRLine()];
        setReceipt(r);
        setLines(l);
        setFile(null);
        setParsed(null);
        setWarnings([]);
        setBaseline(snapshotOf(r, l, null));
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isNew, location.key]);

  useEffect(() => {
    if (full) {
      setReceipt(full.receipt);
      setLines(full.lines);
      setFile(full.file);
      setParsed(full.einvoice);
      setBaseline(snapshotOf(full.receipt, full.lines, full.file));
    }
  }, [full]);

  useEffect(() => {
    if (!payAccount && accounts?.length) {
      const bank = accounts.find((a) => a.account.kind !== 'cash') ?? accounts[0];
      setPayAccount(String(bank.account.id));
    }
  }, [accounts, payAccount]);

  const computed = useMemo(() => (receipt ? lines.map((l) => computeLine(receipt, l)) : []), [receipt, lines]);
  const totals = useMemo(
    () => ({
      net: r2(computed.reduce((a, l) => a + l.net, 0)),
      tax: r2(computed.reduce((a, l) => a + l.tax, 0)),
      gross: r2(computed.reduce((a, l) => a + l.gross, 0)),
    }),
    [computed],
  );

  const save = useAction(
    async (opts: { andNew?: boolean; leaving?: boolean }) => {
      if (!receipt) throw new Error('Kein Beleg');
      const payload: ReceiptInput = {
        receipt: { ...receipt, file_id: file?.id ?? null },
        lines: computed,
        payment: isNew && paidNow && payAccount ? { account_id: Number(payAccount), transaction_id: null, date: payDate, amount: 0, fee_amount: 0, writeoff: '', note: '' } : null,
      };
      const r = isNew ? await api.post<ReceiptFull>('/receipts', payload) : await api.put<ReceiptFull>(`/receipts/${receipt.id}`, payload);
      return { r, andNew: !!opts.andNew, leaving: !!opts.leaving };
    },
    {
      success: ({ r }) => `Beleg ${r.receipt.number} gespeichert`,
      onSuccess: ({ r, andNew, leaving }) => {
        // Clean before any navigation, so the unsaved-changes guard lets it pass.
        setBaseline(null);
        if (leaving) return;
        if (andNew) {
          setTimeout(() => navigate('/belege/neu', { replace: true, state: null }), 0);
          setReceipt(emptyReceipt());
          setLines([emptyRLine()]);
          setFile(null);
          setParsed(null);
          setWarnings([]);
        } else if (isNew) {
          // Import hints only apply while capturing the receipt.
          setWarnings([]);
          setTimeout(() => navigate(`/belege/${r.receipt.id}`, { replace: true, state: null }), 0);
        } else {
          refetch();
        }
      },
    },
  );

  const rcTax = useMemo(() => r2(computed.reduce((a, l) => a + r2((l.net * l.tax_rate) / 100), 0)), [computed]);
  const createSupplier = useAction(
    () =>
      api.post<Contact>('/contacts', {
        ...emptyContact('supplier'),
        company: parsed?.seller.name ?? '',
        street: parsed?.seller.street ?? '',
        zip: parsed?.seller.zip ?? '',
        city: parsed?.seller.city ?? '',
        country: parsed?.seller.country || 'DE',
        vat_id: parsed?.seller.vat_id ?? '',
        tax_number: parsed?.seller.tax_number ?? '',
        email: parsed?.seller.email ?? '',
        phone: parsed?.seller.phone ?? '',
        iban: parsed?.iban ?? '',
        bic: parsed?.bic ?? '',
      }),
    {
      success: (c) => `Lieferant „${c.company}“ angelegt`,
      onSuccess: (c) => {
        setReceipt((r) => (r ? { ...r, contact_id: c.id, supplier_name: c.company } : r));
        setWarnings((w) => w.filter((x) => !x.startsWith('Lieferant')));
      },
    },
  );
  const removePayment = useAction((allocId: number) => api.del(`/allocations/${allocId}`), { success: 'Zahlung entfernt' });
  const remove = useAction(() => api.del(`/receipts/${id}`), {
    success: 'Beleg gelöscht',
    onSuccess: () => {
      setBaseline(null);
      setTimeout(() => navigate('/belege'), 0);
    },
  });

  useHotkeys([['mod+S', () => save.mutate({})]], []);

  const uploadFile = async (f: File) => {
    setUploading(true);
    try {
      const result = await api.upload<ImportResult>('/receipts/import', f);
      if (result.parsed.number && receipt) {
        // E-invoice: take over parsed data, keep categories already chosen
        setReceipt({ ...result.draft.receipt, id: receipt.id, number: receipt.number, receipt_type: receipt.receipt_type });
        setLines(result.draft.lines);
        setParsed(result.parsed);
        setWarnings(result.warnings);
      }
      setFile(result.file);
    } catch (e) {
      notifyError(e, 'Upload fehlgeschlagen');
    } finally {
      setUploading(false);
    }
  };

  if (!receipt || !settings) {
    return (
      <Center h="60vh">
        <Loader />
      </Center>
    );
  }

  const patch = (p: Partial<Receipt>) => setReceipt((r) => (r ? { ...r, ...p } : r));
  const patchLine = (i: number, p: Partial<ReceiptLine>) => setLines((ls) => ls.map((l, j) => (j === i ? { ...l, ...p } : l)));
  const isIncome = receipt.receipt_type === 'income';
  // Supplier credit notes (negative receipts) are settled by a refund.
  const remaining = full ? receiptRemaining(full.receipt, full.open_amount) : 0;
  const refund = !!full && full.receipt.gross_total < 0;
  const accountKind = (n: string) => chart?.find((a) => a.number === n)?.kind;

  const onSupplier = async (cid: number | null, contactName?: string) => {
    patch({ contact_id: cid, supplier_name: contactName ?? receipt.supplier_name });
    if (cid && lines.length === 1 && !lines[0].account) {
      try {
        const s = await api.get<AccountSuggestion | null>('/receipts/suggest', { contact_id: cid });
        if (s) patchLine(0, { account: s.account, tax_rate: s.tax_rate });
      } catch {
        /* ignore */
      }
    }
  };

  return (
    <Page size="full">
      <PageHeader
        back={{ to: '/belege', label: 'Belege' }}
        title={isNew ? 'Beleg erfassen' : `Beleg ${receipt.number}`}
        badge={!isNew && <StatusBadge status={receipt.status} docType="invoice" />}
        subtitle={!isNew ? `${full?.contact ? full.contact.company || `${full.contact.first_name} ${full.contact.last_name}` : receipt.supplier_name} · ${date(receipt.date)}` : 'Tipp: Datei per Drag & Drop hinzufügen – E-Rechnungen füllen das Formular automatisch aus.'}
        actions={
          <>
            {!isNew && (
              <Button variant="subtle" color="red" leftSection={<IconTrash size={16} />} onClick={() => confirmAction({ title: 'Beleg löschen', message: 'Der Beleg und seine Buchungen werden gelöscht.', danger: true, confirmLabel: 'Löschen', onConfirm: () => remove.mutate() })}>
                Löschen
              </Button>
            )}
            {!isNew && remaining > 0.005 && (
              <Button variant="light" color="teal" leftSection={<IconCash size={16} />} onClick={() => setPayOpen(true)}>
                {refund ? 'Erstattung erfassen' : 'Zahlung erfassen'}
              </Button>
            )}
            {isNew && (
              <Button variant="default" leftSection={<IconRefresh size={16} />} onClick={() => save.mutate({ andNew: true })} loading={save.isPending && save.variables?.andNew}>
                Speichern & nächster
              </Button>
            )}
            <Button leftSection={<IconDeviceFloppy size={16} />} onClick={() => save.mutate({})} loading={save.isPending && !save.variables?.andNew}>
              Speichern
            </Button>
          </>
        }
      />
      <Grid gap="lg">
        <Grid.Col span={{ base: 12, lg: 5 }}>
          <FilePanel file={file} onUpload={uploadFile} uploading={uploading} parsed={parsed} />
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 7 }}>
          <Stack gap="lg">
            {warnings.length > 0 && (
              <Alert color="yellow" icon={<IconAlertTriangle size={16} />} title="Bitte prüfen">
                <List size="sm">
                  {warnings.map((w) => (
                    <List.Item key={w}>{w}</List.Item>
                  ))}
                </List>
                {parsed && !receipt.contact_id && parsed.seller.name && (
                  <Button mt="sm" size="xs" variant="white" leftSection={<IconUserPlus size={14} />} loading={createSupplier.isPending} onClick={() => createSupplier.mutate()}>
                    „{parsed.seller.name}“ als Lieferant anlegen
                  </Button>
                )}
              </Alert>
            )}
            <Card>
              <Stack gap="sm">
                <SegmentedControl
                  value={receipt.receipt_type}
                  onChange={(v) => patch({ receipt_type: v })}
                  data={[
                    { value: 'expense', label: 'Ausgabe (Eingangsrechnung, Quittung)' },
                    { value: 'income', label: 'Einnahme (z. B. Plattform-Auszahlung)' },
                  ]}
                />
                <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
                  <Box>
                    <ContactSelect label={isIncome ? 'Geschäftspartner' : 'Lieferant'} role={isIncome ? undefined : 'supplier'} value={receipt.contact_id} onChange={(cid, c) => onSupplier(cid, c ? c.company || `${c.first_name} ${c.last_name}` : undefined)} />
                  </Box>
                  <TextInput label="oder Name (ohne Kontakt)" placeholder="z. B. Tankstelle, Restaurant" value={receipt.supplier_name} onChange={(e) => patch({ supplier_name: e.currentTarget.value })} disabled={!!receipt.contact_id} />
                  <TextInput label="Rechnungs-/Belegnummer" value={receipt.invoice_number} onChange={(e) => patch({ invoice_number: e.currentTarget.value })} />
                  <DateField label="Belegdatum" value={receipt.date} onChange={(v) => v && patch({ date: v })} />
                  <DateField label="Fällig am" value={receipt.due_date} onChange={(v) => patch({ due_date: v })} clearable />
                  <DateField label="Leistungsdatum" value={receipt.service_date} onChange={(v) => patch({ service_date: v })} clearable />
                </SimpleGrid>
                <TextInput label="Beschreibung" placeholder="Wofür war die Ausgabe?" value={receipt.description} onChange={(e) => patch({ description: e.currentTarget.value })} />
              </Stack>
            </Card>

            <Card>
              <Group justify="space-between" mb="sm" wrap="wrap">
                <Text fw={650}>Positionen & Kategorien</Text>
                <Group gap="sm">
                  <Select size="xs" w={300} data={RECEIPT_TAX_MODES} value={receipt.tax_mode} onChange={(v) => patch({ tax_mode: v ?? 'standard' })} aria-label="Steuerart" />
                  <SegmentedControl
                    size="xs"
                    value={receipt.prices_gross ? 'gross' : 'net'}
                    onChange={(v) => patch({ prices_gross: v === 'gross' })}
                    data={[
                      { value: 'gross', label: 'Brutto' },
                      { value: 'net', label: 'Netto' },
                    ]}
                  />
                </Group>
              </Group>
              {receipt.tax_mode !== 'standard' && (
                <Alert variant="light" color="violet" py="xs" mb="sm">
                  {settings.tax.small_business
                    ? 'Die Umsatzsteuer wird geschuldet, ist als Kleinunternehmer aber nicht als Vorsteuer abziehbar.'
                    : 'Umsatzsteuer und Vorsteuer werden automatisch gebucht (Steuerschuldnerschaft des Leistungsempfängers) – der Lieferant stellt netto in Rechnung.'}
                </Alert>
              )}
              <Stack gap="xs">
                {lines.map((l, i) => {
                  const kind = accountKind(l.account);
                  const c = computed[i];
                  return (
                    <Box key={i} p="sm" style={{ border: '1px solid var(--app-border)', borderRadius: 10 }}>
                      <Grid gap="xs" align="flex-end">
                        <Grid.Col span={{ base: 12, sm: 5 }}>
                          <AccountSelect
                            label="Kategorie"
                            picks={isIncome ? ['revenue', 'other'] : ['expense', 'asset', 'tax', 'private', 'other']}
                            value={l.account}
                            onChange={(v, a) => patchLine(i, { account: v ?? '', tax_rate: a?.tax_rate ?? l.tax_rate })}
                          />
                        </Grid.Col>
                        <Grid.Col span={{ base: 12, sm: 7 }}>
                          <TextInput label="Beschreibung" value={l.description} onChange={(e) => patchLine(i, { description: e.currentTarget.value })} />
                        </Grid.Col>
                        <Grid.Col span={{ base: 6, sm: 4 }}>
                          <MoneyInput
                            label={receipt.prices_gross ? 'Betrag brutto' : 'Betrag netto'}
                            value={receipt.prices_gross ? l.gross : l.net}
                            onChange={(v) => patchLine(i, receipt.prices_gross ? { gross: v } : { net: v })}
                          />
                        </Grid.Col>
                        <Grid.Col span={{ base: 6, sm: 3 }}>
                          <Select label="USt" data={[...TAX_RATES, { value: '16', label: '16 %' }, { value: '5', label: '5 %' }]} value={String(l.tax_rate)} onChange={(v) => patchLine(i, { tax_rate: Number(v ?? 19) })} />
                        </Grid.Col>
                        <Grid.Col span={{ base: 10, sm: 4 }}>
                          <Text size="xs" c="dimmed">
                            netto {money(c?.net ?? 0)} · USt {money(c?.tax ?? 0)}
                          </Text>
                          <Text size="sm" fw={600} className="tabular">
                            {money(c?.gross ?? 0)}
                          </Text>
                        </Grid.Col>
                        <Grid.Col span={{ base: 2, sm: 1 }}>
                          <ActionIcon color="red" variant="subtle" onClick={() => setLines((ls) => ls.filter((_, j) => j !== i))} disabled={lines.length === 1} aria-label="Position entfernen">
                            <IconTrash size={16} />
                          </ActionIcon>
                        </Grid.Col>
                        {kind === 'asset' && (
                          <Grid.Col span={12}>
                            <Group gap="sm" p="xs" bg="var(--mantine-color-brand-0)" style={{ borderRadius: 8 }}>
                              <IconBuildingWarehouse size={16} color="var(--mantine-color-brand-7)" />
                              <Text size="sm" style={{ flex: 1 }}>
                                Anlagegut – wird automatisch ins Anlagenverzeichnis übernommen und abgeschrieben.
                              </Text>
                              <NumberInput size="xs" w={150} min={1} max={50} label="Nutzungsdauer (Jahre)" value={l.asset_life_years || ''} placeholder="AfA-Tabelle" onChange={(v) => patchLine(i, { asset_life_years: Number(v) || 0 })} />
                            </Group>
                          </Grid.Col>
                        )}
                      </Grid>
                    </Box>
                  );
                })}
                <Button variant="light" leftSection={<IconPlus size={16} />} w="fit-content" onClick={() => setLines((ls) => [...ls, emptyRLine('', 19)])}>
                  Weitere Position (z. B. anderer Steuersatz)
                </Button>
              </Stack>
              <Divider my="md" />
              <Stack gap={4} maw={320} ml="auto">
                <Group justify="space-between">
                  <Text size="sm" c="dimmed">Netto</Text>
                  <Text size="sm" className="tabular">{money(totals.net)}</Text>
                </Group>
                <Group justify="space-between">
                  <Text size="sm" c="dimmed">{isIncome ? 'Umsatzsteuer' : 'Vorsteuer'}</Text>
                  <Text size="sm" className="tabular">{money(totals.tax)}</Text>
                </Group>
                <Group justify="space-between">
                  <Text fw={700}>Gesamt</Text>
                  <Text fw={700} className="tabular">{money(totals.gross)}</Text>
                </Group>
                {receipt.tax_mode !== 'standard' && !isIncome && (
                  <Text size="xs" c="violet.7" ta="right">
                    zzgl. automatisch gebucht: USt §13b/ig. Erwerb {money(rcTax)}
                    {settings.tax.small_business ? ' (nicht als Vorsteuer abziehbar)' : ' · Vorsteuer ' + money(rcTax)}
                  </Text>
                )}
              </Stack>
            </Card>

            <Card>
              <Text fw={650} mb="sm">
                Zahlung
              </Text>
              {isNew ? (
                <Stack gap="sm">
                  <Switch label={isIncome ? 'Betrag wurde bereits vereinnahmt' : 'Beleg ist bereits bezahlt'} checked={paidNow} onChange={(e) => setPaidNow(e.currentTarget.checked)} />
                  {paidNow && (
                    <SimpleGrid cols={2} spacing="sm">
                      <Select label="Bezahlt über" data={(accounts ?? []).map((a) => ({ value: String(a.account.id), label: a.account.name }))} value={payAccount} onChange={setPayAccount} />
                      <DateField label="Zahlungsdatum" value={payDate} onChange={setPayDate} />
                    </SimpleGrid>
                  )}
                </Stack>
              ) : (
                <Stack gap="xs">
                  {(full?.payments ?? []).length === 0 && (
                    <Text size="sm" c="dimmed">
                      Noch keine {refund ? 'Erstattung' : 'Zahlung'} zugeordnet. Offen: {money(remaining)}
                    </Text>
                  )}
                  {(full?.payments ?? []).map((p) => (
                    <Group key={p.allocation.id} justify="space-between">
                      <Text size="sm">
                        {date(p.allocation.date)} · {p.account_name}
                      </Text>
                      <Group gap="xs">
                        <Text size="sm" fw={600} className="tabular">
                          {money(Math.abs(p.allocation.amount))}
                        </Text>
                        <ActionIcon
                          size="sm"
                          color="red"
                          onClick={() => confirmAction({ title: 'Zahlung entfernen', message: 'Die Zuordnung dieser Zahlung wird entfernt.', danger: true, confirmLabel: 'Entfernen', onConfirm: () => removePayment.mutate(p.allocation.id) })}
                          aria-label="Zahlung entfernen"
                        >
                          <IconTrash size={14} />
                        </ActionIcon>
                      </Group>
                    </Group>
                  ))}
                  {remaining > 0.005 && (
                    <Button variant="light" size="xs" w="fit-content" onClick={() => setPayOpen(true)}>
                      {refund ? 'Erstattung erfassen' : 'Zahlung erfassen'}
                    </Button>
                  )}
                </Stack>
              )}
              {(full?.assets ?? []).length > 0 && (
                <Group gap="xs" mt="sm">
                  {full?.assets.map((a) => (
                    <Badge key={a.id} variant="light" leftSection={<IconBuildingWarehouse size={12} />} style={{ cursor: 'pointer' }} onClick={() => navigate('/buchhaltung/anlagen')}>
                      Anlage {a.number}
                    </Badge>
                  ))}
                </Group>
              )}
              <Textarea mt="md" label="Notizen" value={receipt.notes} onChange={(e) => patch({ notes: e.currentTarget.value })} minRows={2} />
            </Card>
          </Stack>
        </Grid.Col>
      </Grid>
      {full && (
        <PaymentModal
          opened={payOpen}
          onClose={() => {
            setPayOpen(false);
            refetch();
          }}
          target="receipts"
          id={full.receipt.id}
          openAmount={remaining}
          incoming={receiptIncoming(full.receipt)}
          title={`${refund ? 'Erstattung' : 'Zahlung'} für Beleg ${full.receipt.number}`}
        />
      )}
      <UnsavedChangesGuard when={dirty} onSave={() => save.mutateAsync({ leaving: true })} />
    </Page>
  );
}
