// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'react-router';
import {
  ActionIcon,
  Alert,
  Badge,
  Box,
  Button,
  Card,
  Checkbox,
  Divider,
  Drawer,
  FileInput,
  Grid,
  Group,
  Modal,
  Paper,
  SegmentedControl,
  Select,
  SimpleGrid,
  Stack,
  Stepper,
  Table,
  Text,
  TextInput,
  ThemeIcon,
  Tooltip,
  UnstyledButton,
} from '@mantine/core';
import {
  IconArrowsExchange,
  IconBuildingBank,
  IconCash,
  IconCheck,
  IconEdit,
  IconFileImport,
  IconLink,
  IconPlus,
  IconSearch,
  IconSparkles,
  IconTrash,
  IconUpload,
  IconWallet,
} from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction, useApi, useChart, usePaymentAccounts, useSettings } from '@/api/hooks';
import type {
  Allocation,
  CsvMapping,
  DocumentListItem,
  ImportPreview,
  ImportSummary,
  MatchSuggestion,
  PaymentAccount,
  PaymentAccountView,
  ReceiptListItem,
  TransactionView,
} from '@/api/types';
import { AccountSelect, DateField, MoneyInput } from '@/components/inputs';
import { ListTable } from '@/components/ListTable';
import { Empty, Page, PageHeader, confirmAction } from '@/components/ui';
import { receiptIncoming, receiptRemaining } from '@/lib/calc';
import { date, formatIban, money, r2, today } from '@/lib/format';
import { TAX_RATES } from '@/lib/labels';

function emptyAccount(): PaymentAccount {
  return { id: 0, name: '', kind: 'bank', iban: '', bic: '', bank_name: '', ledger_account: '', opening_balance: 0, opening_date: `${new Date().getFullYear()}-01-01`, color: 'blue', archived: false, created_at: '' };
}

export function PaymentAccountModal({ opened, onClose, account }: { opened: boolean; onClose: () => void; account: PaymentAccount | null }) {
  const [a, setA] = useState<PaymentAccount>(account ?? emptyAccount());
  useEffect(() => {
    if (opened) setA(account ?? emptyAccount());
  }, [opened, account]);
  const save = useAction((x: PaymentAccount) => (x.id ? api.put(`/payment-accounts/${x.id}`, x) : api.post('/payment-accounts', x)), { success: 'Konto gespeichert', onSuccess: onClose });
  return (
    <Modal opened={opened} onClose={onClose} title={<Text fw={650}>{a.id ? 'Zahlungskonto bearbeiten' : 'Neues Zahlungskonto'}</Text>}>
      <Stack gap="sm">
        <SegmentedControl
          value={a.kind}
          onChange={(k) => setA({ ...a, kind: k })}
          disabled={!!a.id}
          data={[
            { value: 'bank', label: 'Bankkonto' },
            { value: 'cash', label: 'Kasse' },
            { value: 'other', label: 'PayPal / Sonstige' },
          ]}
        />
        <TextInput label="Bezeichnung" required value={a.name} onChange={(e) => setA({ ...a, name: e.currentTarget.value })} placeholder="z. B. Geschäftskonto Sparkasse" />
        {a.kind !== 'cash' && (
          <>
            <TextInput label="IBAN" value={a.iban} onChange={(e) => setA({ ...a, iban: e.currentTarget.value })} />
            <SimpleGrid cols={2} spacing="sm">
              <TextInput label="BIC" value={a.bic} onChange={(e) => setA({ ...a, bic: e.currentTarget.value })} />
              <TextInput label="Bank" value={a.bank_name} onChange={(e) => setA({ ...a, bank_name: e.currentTarget.value })} />
            </SimpleGrid>
          </>
        )}
        <SimpleGrid cols={2} spacing="sm">
          <MoneyInput label="Anfangsbestand" value={a.opening_balance} onChange={(v) => setA({ ...a, opening_balance: v })} />
          <DateField label="zum" value={a.opening_date} onChange={(v) => v && setA({ ...a, opening_date: v })} />
        </SimpleGrid>
        {a.id > 0 && <Checkbox label="Archiviert" checked={a.archived} onChange={(e) => setA({ ...a, archived: e.currentTarget.checked })} />}
        <Text size="xs" c="dimmed">
          Sachkonto: {a.ledger_account || 'wird automatisch zugeordnet'}
        </Text>
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button onClick={() => save.mutate(a)} loading={save.isPending}>
            Speichern
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

function ManualTxModal({ opened, onClose, accountId }: { opened: boolean; onClose: () => void; accountId: number | null }) {
  const { data: accounts } = usePaymentAccounts();
  const [acc, setAcc] = useState<string | null>(accountId ? String(accountId) : null);
  const [dir, setDir] = useState('in');
  const [amount, setAmount] = useState(0);
  const [day, setDay] = useState<string | null>(today());
  const [party, setParty] = useState('');
  const [purpose, setPurpose] = useState('');
  useEffect(() => {
    if (opened) {
      setAcc(accountId ? String(accountId) : accounts?.find((a) => a.account.kind !== 'cash')?.account.id.toString() ?? null);
      setAmount(0);
      setParty('');
      setPurpose('');
      setDay(today());
    }
  }, [opened, accountId, accounts]);
  const save = useAction(
    () =>
      api.post('/transactions', {
        tx: { id: 0, account_id: Number(acc), date: day, amount: dir === 'in' ? amount : -amount, counterparty: party, counterparty_iban: '', purpose, cash_number: null, status: 'open', import_hash: '', source: 'manual', reversal_of: null, created_at: '' },
        allocations: [],
      }),
    { success: 'Kontobewegung erfasst – jetzt zuordnen', onSuccess: onClose },
  );
  return (
    <Modal opened={opened} onClose={onClose} title={<Text fw={650}>Kontobewegung erfassen</Text>}>
      <Stack gap="sm">
        <Select label="Konto" data={(accounts ?? []).filter((a) => a.account.kind !== 'cash').map((a) => ({ value: String(a.account.id), label: a.account.name }))} value={acc} onChange={setAcc} />
        <SegmentedControl
          value={dir}
          onChange={setDir}
          data={[
            { value: 'in', label: 'Eingang' },
            { value: 'out', label: 'Ausgang' },
          ]}
        />
        <SimpleGrid cols={2} spacing="sm">
          <MoneyInput label="Betrag" value={amount} onChange={setAmount} min={0} />
          <DateField label="Buchungsdatum" value={day} onChange={setDay} />
        </SimpleGrid>
        <TextInput label="Auftraggeber / Empfänger" value={party} onChange={(e) => setParty(e.currentTarget.value)} />
        <TextInput label="Verwendungszweck" value={purpose} onChange={(e) => setPurpose(e.currentTarget.value)} />
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button onClick={() => save.mutate()} disabled={!acc || amount <= 0} loading={save.isPending}>
            Speichern
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

function ImportModal({ opened, onClose, accounts, defaultAccount }: { opened: boolean; onClose: () => void; accounts: PaymentAccountView[]; defaultAccount: number | null }) {
  const [step, setStep] = useState(0);
  const [acc, setAcc] = useState<string | null>(null);
  const [file, setFile] = useState<File | null>(null);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [mapping, setMapping] = useState<CsvMapping | null>(null);
  const [autoMatch, setAutoMatch] = useState(true);
  const [summary, setSummary] = useState<ImportSummary | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const banks = accounts.filter((a) => a.account.kind !== 'cash' && !a.account.archived);

  useEffect(() => {
    if (opened) {
      setStep(0);
      setFile(null);
      setPreview(null);
      setMapping(null);
      setSummary(null);
      setError(null);
      setAcc(defaultAccount ? String(defaultAccount) : banks[0] ? String(banks[0].account.id) : null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opened]);

  const runPreview = async (m: CsvMapping | null) => {
    if (!file || !acc) return;
    setLoading(true);
    setError(null);
    try {
      const p = await api.upload<ImportPreview>('/bank-import/preview', file, { account_id: acc, mapping: m ? JSON.stringify(m) : '' });
      setPreview(p);
      setMapping(p.mapping);
      setStep(1);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  };

  const doImport = useAction(
    () => api.post<ImportSummary>('/bank-import', { account_id: Number(acc), transactions: preview?.transactions ?? [], auto_match: autoMatch }),
    {
      onSuccess: (s) => {
        setSummary(s);
        setStep(2);
      },
    },
  );

  const colOptions = (preview?.columns ?? []).map((c, i) => ({ value: String(i), label: c || `Spalte ${i + 1}` }));
  const mapSelect = (label: string, key: keyof CsvMapping) => (
    <Select
      label={label}
      data={[{ value: '-1', label: '–' }, ...colOptions]}
      value={mapping ? String(mapping[key]) : '-1'}
      onChange={(v) => mapping && setMapping({ ...mapping, [key]: Number(v ?? -1) })}
      size="xs"
    />
  );

  return (
    <Modal opened={opened} onClose={onClose} size={980} title={<Text fw={650}>Kontoauszug importieren</Text>}>
      <Stepper active={step} size="sm" mb="lg">
        <Stepper.Step label="Datei" description="CSV oder CAMT.053" />
        <Stepper.Step label="Prüfen" description="Zuordnung & Vorschau" />
        <Stepper.Step label="Fertig" />
      </Stepper>
      {step === 0 && (
        <Stack>
          <Select label="Bankkonto" data={banks.map((a) => ({ value: String(a.account.id), label: a.account.name }))} value={acc} onChange={setAcc} />
          <FileInput label="Datei" placeholder="Umsätze als CSV (Excel) oder CAMT.053 (XML) auswählen" accept=".csv,.txt,.xml,.camt" value={file} onChange={setFile} leftSection={<IconUpload size={16} />} />
          <Text size="xs" c="dimmed">
            Unterstützt werden die CSV-Exporte gängiger Banken (Sparkasse, Volksbank, ING, DKB, Commerzbank, Deutsche Bank, N26 …) sowie das ISO-20022-Format CAMT.053. Doppelte Umsätze werden automatisch erkannt.
          </Text>
          {error && <Alert color="red">{error}</Alert>}
          <Group justify="flex-end">
            <Button onClick={() => runPreview(null)} disabled={!file || !acc} loading={loading}>
              Weiter
            </Button>
          </Group>
        </Stack>
      )}
      {step === 1 && preview && (
        <Stack>
          {preview.format === 'csv' && mapping && (
            <Paper withBorder p="sm" radius="md">
              <Text size="sm" fw={600} mb="xs">
                Erkannte Spalten
              </Text>
              <SimpleGrid cols={{ base: 2, sm: 4 }} spacing="xs">
                {mapSelect('Datum', 'date_col')}
                {mapSelect('Betrag', 'amount_col')}
                {mapSelect('Name', 'counterparty_col')}
                {mapSelect('Verwendungszweck', 'purpose_col')}
                {mapSelect('IBAN', 'iban_col')}
                {mapSelect('Soll (falls getrennt)', 'debit_col')}
                {mapSelect('Haben (falls getrennt)', 'credit_col')}
              </SimpleGrid>
              <Group justify="flex-end" mt="xs">
                <Button size="xs" variant="light" onClick={() => runPreview(mapping)} loading={loading}>
                  Zuordnung übernehmen
                </Button>
              </Group>
            </Paper>
          )}
          {preview.errors.map((e) => (
            <Alert key={e} color="yellow" py="xs">
              {e}
            </Alert>
          ))}
          <Group gap="sm">
            <Badge size="lg" variant="light">
              {preview.transactions.length} Umsätze erkannt
            </Badge>
            {preview.duplicates > 0 && (
              <Badge size="lg" variant="light" color="gray">
                {preview.duplicates} bereits vorhanden
              </Badge>
            )}
          </Group>
          <Box mah={340} style={{ overflowY: 'auto' }}>
            <Table striped fz="sm" stickyHeader>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>Datum</Table.Th>
                  <Table.Th>Name</Table.Th>
                  <Table.Th>Verwendungszweck</Table.Th>
                  <Table.Th ta="right">Betrag</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {preview.transactions.map((t, i) => (
                  <Table.Tr key={i} style={{ opacity: t.duplicate ? 0.45 : 1 }}>
                    <Table.Td>{date(t.date)}</Table.Td>
                    <Table.Td>{t.counterparty}</Table.Td>
                    <Table.Td>
                      <Text size="xs" lineClamp={1}>
                        {t.purpose}
                      </Text>
                    </Table.Td>
                    <Table.Td ta="right" className="tabular" c={t.amount < 0 ? 'red.7' : 'teal.7'}>
                      {money(t.amount)}
                    </Table.Td>
                  </Table.Tr>
                ))}
              </Table.Tbody>
            </Table>
          </Box>
          <Checkbox label="Eindeutige Zahlungen automatisch Rechnungen und Belegen zuordnen (Rechnungsnummer + Betrag)" checked={autoMatch} onChange={(e) => setAutoMatch(e.currentTarget.checked)} />
          <Group justify="space-between">
            <Button variant="default" onClick={() => setStep(0)}>
              Zurück
            </Button>
            <Button onClick={() => doImport.mutate()} loading={doImport.isPending} disabled={preview.transactions.filter((t) => !t.duplicate).length === 0} leftSection={<IconFileImport size={16} />}>
              {preview.transactions.filter((t) => !t.duplicate).length} Umsätze importieren
            </Button>
          </Group>
        </Stack>
      )}
      {step === 2 && summary && (
        <Stack align="center" py="lg">
          <ThemeIcon size={56} radius="xl" color="teal" variant="light">
            <IconCheck size={30} />
          </ThemeIcon>
          <Text fw={650}>{summary.imported} Umsätze importiert</Text>
          <Text size="sm" c="dimmed">
            {summary.auto_matched} automatisch zugeordnet · {summary.skipped} Duplikate übersprungen
          </Text>
          <Button onClick={onClose}>Schließen</Button>
        </Stack>
      )}
    </Modal>
  );
}

type DraftAlloc = Allocation & { label: string };

function AssignDrawer({ tx, onClose }: { tx: TransactionView | null; onClose: () => void }) {
  const { data: settings } = useSettings();
  const { data: chart } = useChart();
  const transitAccount = chart?.find((a) => a.pick === 'transfer')?.number ?? '1360';
  const { data: suggestions } = useApi<MatchSuggestion[]>(tx ? `/transactions/${tx.tx.id}/suggestions` : null);
  const { data: openDocs } = useApi<DocumentListItem[]>(tx ? '/documents' : null, { status: 'unpaid' });
  const { data: openReceipts } = useApi<ReceiptListItem[]>(tx ? '/receipts' : null, { status: 'unpaid' });
  const [allocs, setAllocs] = useState<DraftAlloc[]>([]);
  const [mode, setMode] = useState('match');
  const [search, setSearch] = useState('');
  const [account, setAccount] = useState<string | null>(null);
  const [rate, setRate] = useState('19');
  const [desc, setDesc] = useState('');
  const [amount, setAmount] = useState(0);

  useEffect(() => {
    if (tx) {
      setAllocs(tx.allocations.map((a) => ({ ...a.allocation, label: a.label })));
      setMode('match');
      setSearch('');
      setDesc(tx.tx.purpose);
      setAccount(null);
    }
  }, [tx]);

  const allocated = r2(allocs.reduce((s, a) => s + a.amount, 0));
  const rest = tx ? r2(tx.tx.amount - allocated) : 0;
  useEffect(() => setAmount(Math.abs(rest)), [rest]);

  const save = useAction(() => api.put(`/transactions/${tx!.tx.id}`, { tx: tx!.tx, allocations: allocs.map(({ label, ...a }) => a) }), { success: 'Zuordnung gespeichert', onSuccess: onClose });
  const del = useAction(() => api.del(`/transactions/${tx!.tx.id}`), { success: 'Kontobewegung gelöscht', onSuccess: onClose });

  if (!tx) return <Drawer opened={false} onClose={onClose} />;
  const incoming = tx.tx.amount > 0;
  const sign = incoming ? 1 : -1;

  const addTarget = (kind: 'document' | 'receipt', id: number, open: number, label: string) => {
    const amt = Math.min(open, Math.abs(rest));
    if (amt <= 0) return;
    setAllocs((a) => [
      ...a,
      { id: 0, transaction_id: tx.tx.id, kind, document_id: kind === 'document' ? id : null, receipt_id: kind === 'receipt' ? id : null, account: '', tax_rate: 0, amount: sign * amt, date: tx.tx.date, writeoff_reason: '', description: '', created_at: '', label },
    ]);
  };

  const searchResults = (() => {
    const s = search.trim().toLowerCase();
    const docs = (openDocs ?? [])
      .filter((d) => d.status !== 'draft' && (incoming ? d.doc_type === 'invoice' : d.doc_type === 'credit_note'))
      .filter((d) => !s || `${d.number} ${d.contact_name} ${d.open_amount}`.toLowerCase().includes(s))
      .map((d) => ({ kind: 'document' as const, id: d.id, label: `${d.number} · ${d.contact_name}`, open: d.open_amount, date: d.date }));
    const recs = (openReceipts ?? [])
      // Supplier credit notes are settled by incoming money.
      .filter((r) => receiptIncoming(r.receipt) === incoming && receiptRemaining(r.receipt, r.open_amount) > 0)
      .filter((r) => !s || `${r.receipt.number} ${r.receipt.invoice_number} ${r.party} ${Math.abs(r.open_amount)}`.toLowerCase().includes(s))
      .map((r) => ({ kind: 'receipt' as const, id: r.receipt.id, label: `${r.receipt.gross_total < 0 ? 'Gutschrift' : 'Beleg'} ${r.receipt.number} · ${r.party}`, open: receiptRemaining(r.receipt, r.open_amount), date: r.receipt.date }));
    return [...docs, ...recs].slice(0, 12);
  })();

  return (
    <Drawer opened={!!tx} onClose={onClose} size="xl" title={<Text fw={650}>Kontobewegung zuordnen</Text>}>
      <Stack gap="md">
        <Paper withBorder p="md" radius="md">
          <Group justify="space-between" align="flex-start">
            <Box>
              <Text fw={650}>{tx.tx.counterparty || 'Ohne Namen'}</Text>
              <Text size="sm" c="dimmed">
                {date(tx.tx.date)} · {tx.account_name}
                {tx.tx.counterparty_iban ? ` · ${formatIban(tx.tx.counterparty_iban)}` : ''}
              </Text>
              <Text size="sm" mt={4}>
                {tx.tx.purpose}
              </Text>
            </Box>
            <Text fz={24} fw={700} className="tabular" c={incoming ? 'teal.7' : 'red.7'}>
              {money(tx.tx.amount)}
            </Text>
          </Group>
        </Paper>

        {allocs.length > 0 && (
          <Stack gap={6}>
            <Text size="sm" fw={600}>
              Zuordnungen
            </Text>
            {allocs.map((a, i) => (
              <Group key={i} justify="space-between" p="xs" style={{ border: '1px solid var(--app-border)', borderRadius: 8 }}>
                <Group gap="xs">
                  <ThemeIcon size="sm" variant="light" color="teal">
                    <IconLink size={12} />
                  </ThemeIcon>
                  <Text size="sm">{a.label}</Text>
                </Group>
                <Group gap="xs">
                  <Text size="sm" fw={600} className="tabular">
                    {money(a.amount)}
                  </Text>
                  <ActionIcon size="sm" color="red" onClick={() => setAllocs((x) => x.filter((_, j) => j !== i))} aria-label="Zuordnung entfernen">
                    <IconTrash size={14} />
                  </ActionIcon>
                </Group>
              </Group>
            ))}
          </Stack>
        )}
        <Group justify="space-between">
          <Text size="sm" c="dimmed">
            Noch zuzuordnen
          </Text>
          <Text fw={650} className="tabular" c={Math.abs(rest) < 0.005 ? 'teal.7' : 'orange.8'}>
            {money(rest)}
          </Text>
        </Group>

        {Math.abs(rest) >= 0.005 && (
          <>
            <SegmentedControl
              value={mode}
              onChange={setMode}
              data={[
                { value: 'match', label: incoming ? 'Rechnung / Beleg' : 'Beleg / Gutschrift' },
                { value: 'account', label: 'Direkt buchen' },
                { value: 'transfer', label: 'Umbuchung' },
              ]}
            />
            {mode === 'match' && (
              <Stack gap="xs">
                {(suggestions ?? []).length > 0 && (
                  <>
                    <Group gap={6}>
                      <IconSparkles size={16} color="var(--mantine-color-violet-6)" />
                      <Text size="sm" fw={600}>
                        Vorschläge
                      </Text>
                    </Group>
                    {(suggestions ?? []).map((s) => (
                      <UnstyledButton key={`${s.kind}-${s.id}`} onClick={() => addTarget(s.kind as 'document' | 'receipt', s.id, s.open_amount, `${s.kind === 'document' ? 'Rechnung' : 'Beleg'} ${s.number} · ${s.name}`)}>
                        <Paper withBorder p="sm" radius="md" style={{ borderColor: s.score >= 80 ? 'var(--mantine-color-teal-4)' : undefined }}>
                          <Group justify="space-between" wrap="nowrap">
                            <Box>
                              <Text size="sm" fw={600}>
                                {s.kind === 'document' ? 'Rechnung' : 'Beleg'} {s.number} · {s.name}
                              </Text>
                              <Text size="xs" c="dimmed">
                                {date(s.date)} · {s.reason}
                              </Text>
                            </Box>
                            <Group gap="xs" wrap="nowrap">
                              <Badge color={s.score >= 80 ? 'teal' : s.score >= 40 ? 'yellow' : 'gray'} variant="light">
                                {Math.min(100, s.score)} %
                              </Badge>
                              <Text size="sm" fw={600} className="tabular">
                                {money(s.open_amount)}
                              </Text>
                            </Group>
                          </Group>
                        </Paper>
                      </UnstyledButton>
                    ))}
                  </>
                )}
                <TextInput placeholder="Offene Rechnungen und Belege durchsuchen …" leftSection={<IconSearch size={16} />} value={search} onChange={(e) => setSearch(e.currentTarget.value)} />
                {searchResults.map((r) => (
                  <Group key={`${r.kind}-${r.id}`} justify="space-between" px="xs">
                    <Text size="sm">{r.label}</Text>
                    <Group gap="xs">
                      <Text size="sm" className="tabular">
                        {money(r.open)}
                      </Text>
                      <Button size="compact-xs" variant="light" onClick={() => addTarget(r.kind, r.id, r.open, r.label)}>
                        Zuordnen
                      </Button>
                    </Group>
                  </Group>
                ))}
              </Stack>
            )}
            {(mode === 'account' || mode === 'transfer') && (
              <Stack gap="sm">
                {mode === 'account' ? (
                  <AccountSelect
                    label="Konto / Kategorie"
                    picks={incoming ? ['revenue', 'private', 'tax', 'other'] : ['expense', 'private', 'tax', 'asset', 'other']}
                    value={account}
                    onChange={(v, a) => {
                      setAccount(v);
                      if (a?.tax_rate != null) setRate(String(a.tax_rate));
                    }}
                  />
                ) : (
                  <Alert variant="light" icon={<IconArrowsExchange size={16} />}>
                    Umbuchung zwischen eigenen Konten (z. B. Bargeldabhebung für die Kasse) – wird über das Geldtransitkonto gebucht. Erfassen Sie die Gegenbuchung im anderen Konto bzw. im Kassenbuch ebenfalls als Umbuchung.
                  </Alert>
                )}
                <SimpleGrid cols={2} spacing="sm">
                  <MoneyInput label="Betrag" value={amount} onChange={setAmount} min={0} />
                  {mode === 'account' && <Select label="USt-Satz" data={TAX_RATES} value={rate} onChange={(v) => setRate(v ?? '0')} disabled={settings?.tax.small_business} />}
                </SimpleGrid>
                <TextInput label="Buchungstext" value={desc} onChange={(e) => setDesc(e.currentTarget.value)} />
                <Button
                  variant="light"
                  leftSection={<IconPlus size={16} />}
                  w="fit-content"
                  disabled={amount <= 0 || (mode === 'account' && !account)}
                  onClick={() => {
                    const acc = mode === 'transfer' ? transitAccount : account!;
                    setAllocs((a) => [
                      ...a,
                      { id: 0, transaction_id: tx.tx.id, kind: 'account', document_id: null, receipt_id: null, account: acc, tax_rate: mode === 'transfer' ? 0 : Number(rate), amount: sign * Math.min(amount, Math.abs(rest)), date: tx.tx.date, writeoff_reason: '', description: desc, created_at: '', label: mode === 'transfer' ? `Geldtransit ${acc}` : `${acc} · ${desc}` },
                    ]);
                  }}
                >
                  Hinzufügen
                </Button>
              </Stack>
            )}
          </>
        )}
        <Divider />
        <Group justify="space-between">
          {tx.tx.source !== 'cash' ? (
            <Button variant="subtle" color="red" leftSection={<IconTrash size={16} />} onClick={() => confirmAction({ title: 'Kontobewegung löschen', message: 'Die Kontobewegung und alle Zuordnungen werden gelöscht.', danger: true, confirmLabel: 'Löschen', onConfirm: () => del.mutate() })}>
              Löschen
            </Button>
          ) : (
            <span />
          )}
          <Group>
            <Button variant="default" onClick={onClose}>
              Abbrechen
            </Button>
            <Button onClick={() => save.mutate()} loading={save.isPending} leftSection={<IconCheck size={16} />}>
              Speichern
            </Button>
          </Group>
        </Group>
      </Stack>
    </Drawer>
  );
}

export function BankPage() {
  const [params, setParams] = useSearchParams();
  const { data: accounts } = usePaymentAccounts();
  const accountId = params.get('konto') ? Number(params.get('konto')) : null;
  const [status, setStatus] = useState('all');
  const [q, setQ] = useState('');
  const { data: txs, isFetching } = useApi<TransactionView[]>('/transactions', { account_id: accountId ?? undefined, status: status === 'all' ? undefined : status, q: q || undefined }, { keepPrevious: true });
  const [assign, setAssign] = useState<TransactionView | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const [accEdit, setAccEdit] = useState<{ open: boolean; account: PaymentAccount | null }>({ open: false, account: null });
  const banks = useMemo(() => (accounts ?? []).filter((a) => !a.account.archived), [accounts]);
  const openCount = (txs ?? []).filter((t) => t.tx.status === 'open').length;

  return (
    <Page>
      <PageHeader
        title="Bank & Zahlungen"
        subtitle="Kontobewegungen importieren und Rechnungen, Belegen oder Konten zuordnen"
        actions={
          <>
            <Button variant="default" leftSection={<IconPlus size={16} />} onClick={() => setManualOpen(true)}>
              Bewegung erfassen
            </Button>
            <Button leftSection={<IconFileImport size={16} />} onClick={() => setImportOpen(true)}>
              Kontoauszug importieren
            </Button>
          </>
        }
      />
      <Grid gap="lg">
        <Grid.Col span={{ base: 12, lg: 3 }}>
          <Stack gap="sm">
            <UnstyledButton onClick={() => setParams({})}>
              <Card padding="md" style={{ outline: accountId === null ? '2px solid var(--mantine-color-brand-5)' : undefined }}>
                <Group gap="sm">
                  <ThemeIcon variant="light" radius="md">
                    <IconWallet size={18} />
                  </ThemeIcon>
                  <Box>
                    <Text size="sm" fw={600}>
                      Alle Konten
                    </Text>
                    <Text size="sm" className="tabular" fw={700}>
                      {money(banks.reduce((s, a) => s + a.balance, 0))}
                    </Text>
                  </Box>
                </Group>
              </Card>
            </UnstyledButton>
            {banks.map((a) => (
              <UnstyledButton key={a.account.id} onClick={() => setParams({ konto: String(a.account.id) })}>
                <Card padding="md" style={{ outline: accountId === a.account.id ? '2px solid var(--mantine-color-brand-5)' : undefined }}>
                  <Group justify="space-between" wrap="nowrap" align="flex-start">
                    <Group gap="sm" wrap="nowrap">
                      <ThemeIcon variant="light" radius="md" color={a.account.kind === 'cash' ? 'teal' : a.account.kind === 'other' ? 'grape' : 'brand'}>
                        {a.account.kind === 'cash' ? <IconCash size={18} /> : <IconBuildingBank size={18} />}
                      </ThemeIcon>
                      <Box style={{ minWidth: 0 }}>
                        <Text size="sm" fw={600} truncate>
                          {a.account.name}
                        </Text>
                        <Text size="xs" c="dimmed" truncate>
                          {a.account.iban ? formatIban(a.account.iban) : `Konto ${a.account.ledger_account}`}
                        </Text>
                        <Text size="sm" fw={700} className="tabular" c={a.balance < 0 ? 'red.7' : undefined}>
                          {money(a.balance)}
                        </Text>
                        {a.open_count > 0 && (
                          <Badge size="xs" mt={4} variant="light">
                            {a.open_count} offen
                          </Badge>
                        )}
                      </Box>
                    </Group>
                    <Tooltip label="Bearbeiten">
                      <ActionIcon size="sm" onClick={(e) => { e.stopPropagation(); setAccEdit({ open: true, account: a.account }); }} aria-label="Konto bearbeiten">
                        <IconEdit size={14} />
                      </ActionIcon>
                    </Tooltip>
                  </Group>
                </Card>
              </UnstyledButton>
            ))}
            <Button variant="subtle" leftSection={<IconPlus size={16} />} onClick={() => setAccEdit({ open: true, account: null })}>
              Konto hinzufügen
            </Button>
          </Stack>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 9 }}>
          <Stack gap="md">
            <Group justify="space-between">
              <SegmentedControl
                value={status}
                onChange={setStatus}
                data={[
                  { value: 'all', label: 'Alle' },
                  { value: 'open', label: `Nicht zugeordnet${status !== 'open' && openCount ? ` (${openCount})` : ''}` },
                  { value: 'booked', label: 'Zugeordnet' },
                ]}
              />
              <TextInput placeholder="Name, Zweck, Betrag …" leftSection={<IconSearch size={16} />} value={q} onChange={(e) => setQ(e.currentTarget.value)} w={260} />
            </Group>
            <Card padding={0}>
              {txs && txs.length === 0 && status === 'all' && !q ? (
                <Empty
                  icon={<IconBuildingBank size={22} />}
                  title="Noch keine Kontobewegungen"
                  description="Importieren Sie Ihre Kontoumsätze als CSV oder CAMT-Datei – Zahlungen werden automatisch Rechnungen zugeordnet."
                  action={
                    <Button leftSection={<IconFileImport size={16} />} onClick={() => setImportOpen(true)}>
                      Kontoauszug importieren
                    </Button>
                  }
                />
              ) : (
                <ListTable<TransactionView>
                  records={txs}
                  idAccessor={(t) => t.tx.id}
                  fetching={isFetching}
                  defaultSort={{ column: 'tx.date', direction: 'desc' }}
                  onRowClick={setAssign}
                  columns={[
                    { accessor: 'tx.date', title: 'Datum', sortable: true, width: 105, render: (t) => date(t.tx.date) },
                    {
                      accessor: 'tx.counterparty',
                      title: 'Name / Verwendungszweck',
                      render: (t) => (
                        <Box style={{ minWidth: 0 }}>
                          <Text size="sm" fw={500} truncate>
                            {t.tx.counterparty || t.tx.purpose}
                          </Text>
                          <Text size="xs" c="dimmed" lineClamp={1}>
                            {[t.tx.counterparty ? t.tx.purpose : '', accountId === null ? t.account_name : ''].filter(Boolean).join(' · ')}
                          </Text>
                        </Box>
                      ),
                    },
                    {
                      accessor: 'allocations',
                      title: 'Zuordnung',
                      visibleMediaQuery: '(min-width: 62em)',
                      render: (t) =>
                        t.allocations.length === 0 ? (
                          <Badge variant="light" color="blue">
                            nicht zugeordnet
                          </Badge>
                        ) : (
                          <Text size="xs" c="dimmed" lineClamp={2}>
                            {t.allocations.map((a) => a.label).join(' · ')}
                          </Text>
                        ),
                    },
                    {
                      accessor: 'tx.amount',
                      title: 'Betrag',
                      sortable: true,
                      textAlign: 'right' as const,
                      render: (t: TransactionView) => (
                        <Text size="sm" fw={600} className="tabular" c={t.tx.amount < 0 ? 'red.7' : 'teal.7'}>
                          {money(t.tx.amount)}
                        </Text>
                      ),
                    },
                  ]}
                />
              )}
            </Card>
          </Stack>
        </Grid.Col>
      </Grid>
      <AssignDrawer tx={assign} onClose={() => setAssign(null)} />
      <ImportModal opened={importOpen} onClose={() => setImportOpen(false)} accounts={accounts ?? []} defaultAccount={accountId} />
      <ManualTxModal opened={manualOpen} onClose={() => setManualOpen(false)} accountId={accountId} />
      <PaymentAccountModal opened={accEdit.open} onClose={() => setAccEdit({ open: false, account: null })} account={accEdit.account} />
    </Page>
  );
}
