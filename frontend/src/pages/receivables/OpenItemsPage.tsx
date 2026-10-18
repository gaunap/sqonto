// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useMemo, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router';
import { ActionIcon, Badge, Button, Card, Group, Paper, SimpleGrid, Stack, Tabs, Text, Tooltip, UnstyledButton } from '@mantine/core';
import { IconBellRinging, IconBuildingBank, IconCash, IconDownload, IconExternalLink, IconFileExport, IconTrash } from '@tabler/icons-react';
import { api, downloadFile, openFile } from '@/api/client';
import { notifyError, useAction, useApi, useSettings } from '@/api/hooks';
import type { AgingBucket, DebitCandidate, DocumentListItem, DunningListItem, DunningSuggestion, OpenItems, PaymentRunView, ReceiptListItem, TransferCandidate } from '@/api/types';
import { DebitTab } from '@/components/DebitRuns';
import { ListTable } from '@/components/ListTable';
import { PaymentModal } from '@/components/PaymentModal';
import { TransferModal } from '@/components/TransferModal';
import { Page, PageHeader, StatusBadge, confirmAction } from '@/components/ui';
import { exportCsv } from '@/lib/csv';
import { documentIncoming, documentRemaining, receiptIncoming, receiptRemaining } from '@/lib/calc';
import { date, money } from '@/lib/format';
import { docLabel } from '@/lib/labels';

const BUCKET_COLORS: Record<string, string> = { current: 'blue', d30: 'yellow', d60: 'orange', d90: 'red', d90p: 'red' };

function Buckets({ buckets, active, onSelect }: { buckets: AgingBucket[]; active: string | null; onSelect: (k: string | null) => void }) {
  return (
    <SimpleGrid cols={{ base: 2, sm: 5 }} spacing="sm">
      {buckets.map((b) => (
        <UnstyledButton key={b.key} onClick={() => onSelect(active === b.key ? null : b.key)}>
          <Paper withBorder p="sm" radius="md" style={{ borderTop: `3px solid var(--mantine-color-${BUCKET_COLORS[b.key]}-5)`, outline: active === b.key ? '2px solid var(--mantine-color-brand-5)' : undefined }}>
            <Text size="xs" c="dimmed">
              {b.label}
            </Text>
            <Text fw={700} className="tabular">
              {money(b.amount)}
            </Text>
            <Text size="xs" c="dimmed">
              {b.count} Posten
            </Text>
          </Paper>
        </UnstyledButton>
      ))}
    </SimpleGrid>
  );
}

function inBucket(days: number, key: string | null) {
  if (!key) return true;
  if (key === 'current') return days <= 0;
  if (key === 'd30') return days >= 1 && days <= 30;
  if (key === 'd60') return days >= 31 && days <= 60;
  if (key === 'd90') return days >= 61 && days <= 90;
  return days > 90;
}

export function OpenItemsPage() {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const tab = params.get('tab') ?? 'forderungen';
  const { data, isFetching } = useApi<OpenItems>('/reports/open-items');
  const { data: suggestions } = useApi<DunningSuggestion[]>('/dunning/suggestions', { all: true });
  const { data: dunnings } = useApi<DunningListItem[]>('/dunning');
  const [bucket, setBucket] = useState<string | null>(null);
  const [pBucket, setPBucket] = useState<string | null>(null);
  const [selected, setSelected] = useState<DocumentListItem[]>([]);
  const { data: candidates } = useApi<TransferCandidate[]>('/payment-runs/candidates');
  const { data: runs } = useApi<PaymentRunView[]>('/payment-runs');
  const { data: debits } = useApi<DebitCandidate[]>('/debit-runs/candidates');
  const debitsReady = (debits ?? []).filter((c) => !c.problem && !c.pending).length;
  const { data: settings } = useSettings();
  const showDebits = !!settings?.debit.creditor_id.trim() || (debits ?? []).length > 0 || tab === 'lastschriften';
  const [selectedPayables, setSelectedPayables] = useState<ReceiptListItem[]>([]);
  const [transferOpen, setTransferOpen] = useState(false);
  const candidateOf = useMemo(() => new Map((candidates ?? []).map((c) => [c.receipt_id, c])), [candidates]);
  const removeRun = useAction((id: number) => api.del(`/payment-runs/${id}`), { success: 'Überweisungsdatei verworfen' });
  const [paying, setPaying] = useState<{ kind: 'documents' | 'receipts'; id: number; open: number; title: string; fees?: number; incoming?: boolean } | null>(null);

  const receivables = useMemo(() => (data?.receivables ?? []).filter((d) => inBucket(d.days_overdue, bucket)), [data, bucket]);
  const payables = useMemo(
    () => (data?.payables ?? []).filter((r) => inBucket(r.receipt.due_date ? Math.floor((Date.now() - new Date(r.receipt.due_date).getTime()) / 86400000) : 0, pBucket)),
    [data, pBucket],
  );
  const dueSuggestions = (suggestions ?? []).filter((s) => s.due);

  const runDunning = useAction(
    async (docs: { id: number; level: number }[]) => {
      const created: number[] = [];
      for (const d of docs) {
        const r = await api.post<{ id: number }>('/dunning', { document_id: d.id, level: d.level });
        created.push(r.id);
      }
      return created;
    },
    {
      success: (ids) => `${ids.length} Mahnung(en) erstellt`,
      onSuccess: (ids) => {
        setSelected([]);
        ids.slice(0, 5).forEach((id) => openFile(`/dunning/${id}/pdf`));
      },
    },
  );
  const removeDunning = useAction((id: number) => api.del(`/dunning/${id}`), { success: 'Mahnung gelöscht' });

  const levelFor = (docId: number) => suggestions?.find((s) => s.document.id === docId);

  return (
    <Page>
      <PageHeader
        title="Offene Posten & Mahnwesen"
        subtitle="Forderungen und Verbindlichkeiten im Blick behalten, Zahlungen zuordnen und säumige Kunden mahnen"
        actions={
          <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile('/reports/pdf/open-items').catch((e) => notifyError(e))}>
            OP-Liste (PDF)
          </Button>
        }
      />
      <Tabs value={tab} onChange={(v) => setParams(v === 'forderungen' ? {} : { tab: v ?? '' })}>
        <Tabs.List mb="md">
          <Tabs.Tab value="forderungen" rightSection={<Badge size="sm" variant="light">{money(data?.receivables_total ?? 0)}</Badge>}>
            Forderungen
          </Tabs.Tab>
          <Tabs.Tab value="verbindlichkeiten" rightSection={<Badge size="sm" variant="light" color="grape">{money(data?.payables_total ?? 0)}</Badge>}>
            Verbindlichkeiten
          </Tabs.Tab>
          {showDebits ? (
            <Tabs.Tab value="lastschriften" rightSection={debitsReady ? <Badge size="sm">{debitsReady}</Badge> : undefined}>
              Lastschriften
            </Tabs.Tab>
          ) : null}
          <Tabs.Tab value="mahnwesen" rightSection={dueSuggestions.length ? <Badge size="sm" color="orange">{dueSuggestions.length}</Badge> : undefined}>
            Mahnwesen
          </Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="forderungen">
          <Stack gap="md">
            {data && <Buckets buckets={data.receivable_buckets} active={bucket} onSelect={setBucket} />}
            <Group justify="space-between">
              <Text size="sm" c="dimmed">
                {selected.length > 0 ? `${selected.length} ausgewählt` : 'Wählen Sie überfällige Rechnungen für einen Mahnlauf aus.'}
              </Text>
              <Group gap="xs">
                <Button
                  variant="default"
                  leftSection={<IconFileExport size={16} />}
                  onClick={() =>
                    exportCsv('Offene_Forderungen', receivables, [
                      { label: 'Nummer', value: (d) => d.number },
                      { label: 'Kunde', value: (d) => d.contact_name },
                      { label: 'Datum', value: (d) => date(d.date) },
                      { label: 'Fällig', value: (d) => date(d.due_date) },
                      { label: 'Tage überfällig', value: (d) => d.days_overdue },
                      { label: 'Offen', value: (d) => d.open_amount },
                    ])
                  }
                >
                  CSV
                </Button>
                <Button
                  color="orange"
                  leftSection={<IconBellRinging size={16} />}
                  disabled={selected.filter((d) => d.days_overdue > 0).length === 0}
                  loading={runDunning.isPending}
                  onClick={() => {
                    const docs = selected.filter((d) => d.days_overdue > 0).map((d) => ({ id: d.id, level: levelFor(d.id)?.next_level ?? 0 }));
                    confirmAction({
                      title: 'Mahnlauf starten',
                      message: `Für ${docs.length} Rechnung(en) wird die jeweils nächste Mahnstufe erstellt und als PDF geöffnet.`,
                      confirmLabel: 'Mahnungen erstellen',
                      onConfirm: () => runDunning.mutate(docs),
                    });
                  }}
                >
                  Mahnungen erstellen
                </Button>
              </Group>
            </Group>
            <Card padding={0}>
              <ListTable<DocumentListItem>
                records={receivables}
                fetching={isFetching}
                defaultSort={{ column: 'days_overdue', direction: 'desc' }}
                onRowClick={(d) => navigate(`/dokumente/${d.id}`)}
                selectedRecords={selected}
                onSelectedRecordsChange={setSelected}
                isRecordSelectable={(d) => d.days_overdue > 0 && d.doc_type === 'invoice'}
                emptyText="Keine offenen Forderungen 🎉"
                columns={[
                  { accessor: 'number', title: 'Rechnung', sortable: true, render: (d) => <><Text size="sm" fw={600}>{d.number}</Text>{d.kind !== 'standard' && <Text size="xs" c="dimmed">{docLabel(d.doc_type, d.kind)}</Text>}</> },
                  { accessor: 'contact_name', title: 'Kunde', sortable: true },
                  { accessor: 'date', title: 'Datum', sortable: true, render: (d) => date(d.date) },
                  { accessor: 'due_date', title: 'Fällig', sortable: true, render: (d) => date(d.due_date) },
                  {
                    accessor: 'days_overdue',
                    title: 'Überfällig',
                    sortable: true,
                    render: (d) => (d.days_overdue > 0 ? <Badge color={d.days_overdue > 30 ? 'red' : 'orange'} variant="light">{d.days_overdue} Tage</Badge> : <Text size="sm" c="dimmed">–</Text>),
                  },
                  {
                    accessor: 'dunning_count',
                    title: 'Mahnstufe',
                    sortable: true,
                    render: (d) => {
                      const s = levelFor(d.id);
                      return (
                        <Group gap={4}>
                          {d.dunning_count > 0 ? (
                            <Tooltip label={`${d.dunning_count} Mahnschreiben erstellt`}>
                              <Badge variant="dot" color="orange">
                                {s?.last_dunning?.title || `${d.dunning_count}× gemahnt`}
                              </Badge>
                            </Tooltip>
                          ) : (
                            <Text size="xs" c="dimmed">
                              –
                            </Text>
                          )}
                          {s?.due && <Tooltip label={`Fällig: ${s.level_name}`}><Badge size="xs" color="orange">fällig</Badge></Tooltip>}
                        </Group>
                      );
                    },
                  },
                  { accessor: 'open_amount', title: 'Offen', sortable: true, textAlign: 'right', render: (d) => <Text size="sm" fw={600} className="tabular" c={d.doc_type === 'credit_note' ? 'grape.7' : undefined}>{d.doc_type === 'credit_note' ? '−' : ''}{money(d.open_amount)}</Text> },
                  {
                    accessor: 'pay',
                    title: '',
                    width: 60,
                    render: (d) => (
                      <Tooltip label="Zahlung erfassen">
                        <ActionIcon color="teal" variant="light" onClick={(e) => { e.stopPropagation(); setPaying({ kind: 'documents', id: d.id, open: documentRemaining(d, d.open_amount), title: `${documentIncoming(d) ? 'Zahlung' : 'Erstattung'} für ${d.number}`, incoming: documentIncoming(d) }); }} aria-label="Zahlung erfassen">
                          <IconCash size={16} />
                        </ActionIcon>
                      </Tooltip>
                    ),
                  },
                ]}
              />
            </Card>
          </Stack>
        </Tabs.Panel>

        <Tabs.Panel value="verbindlichkeiten">
          <Stack gap="md">
            {data && <Buckets buckets={data.payable_buckets} active={pBucket} onSelect={setPBucket} />}
            <Card padding={0}>
              <Group justify="space-between" p="md">
                <Text size="sm" c="dimmed">
                  {selectedPayables.length
                    ? `${selectedPayables.length} ausgewählt · ${money(selectedPayables.reduce((s, r) => s + r.open_amount, 0))}`
                    : 'Belege auswählen, um eine SEPA-Überweisungsdatei zu erstellen'}
                </Text>
                <Button leftSection={<IconBuildingBank size={16} />} disabled={selectedPayables.length === 0} onClick={() => setTransferOpen(true)}>
                  Überweisungen erstellen{selectedPayables.length ? ` (${selectedPayables.length})` : ''}
                </Button>
              </Group>
              <ListTable<ReceiptListItem>
                records={payables}
                idAccessor={(r) => r.receipt.id}
                selectedRecords={selectedPayables}
                onSelectedRecordsChange={setSelectedPayables}
                isRecordSelectable={(r) => candidateOf.has(r.receipt.id)}
                defaultSort={{ column: 'receipt.due_date', direction: 'asc' }}
                onRowClick={(r) => navigate(`/belege/${r.receipt.id}`)}
                emptyText="Keine offenen Verbindlichkeiten"
                columns={[
                  { accessor: 'receipt.number', title: 'Beleg', render: (r) => <><Text size="sm" fw={600}>{r.receipt.number}</Text><Text size="xs" c="dimmed">{r.receipt.invoice_number}</Text></> },
                  { accessor: 'party', title: 'Lieferant', sortable: true },
                  { accessor: 'receipt.date', title: 'Datum', sortable: true, render: (r) => date(r.receipt.date) },
                  { accessor: 'receipt.due_date', title: 'Fällig', sortable: true, render: (r) => <Text size="sm" c={r.overdue ? 'red.7' : undefined}>{date(r.receipt.due_date)}</Text> },
                  {
                    accessor: 'receipt.status',
                    title: 'Status',
                    render: (r) => {
                      const pending = candidateOf.get(r.receipt.id)?.pending;
                      return (
                        <Group gap={4} wrap="nowrap">
                          <StatusBadge status={r.receipt.status} docType="invoice" daysOverdue={r.overdue ? 1 : 0} size="sm" />
                          {pending && (
                            <Tooltip label={`In einer Überweisungsdatei zur Ausführung am ${date(pending)}`}>
                              <Badge size="sm" variant="light" color="blue" leftSection={<IconBuildingBank size={11} />}>
                                {date(pending)}
                              </Badge>
                            </Tooltip>
                          )}
                        </Group>
                      );
                    },
                  },
                  { accessor: 'open_amount', title: 'Offen', sortable: true, textAlign: 'right', render: (r) => <Text size="sm" fw={600} className="tabular" c={r.open_amount < 0 ? 'grape.7' : undefined}>{money(r.open_amount)}</Text> },
                  {
                    accessor: 'pay',
                    title: '',
                    width: 60,
                    render: (r) => (
                      <Tooltip label={r.open_amount < 0 ? 'Erstattung erfassen' : 'Als bezahlt erfassen'}>
                        <ActionIcon
                          color="teal"
                          variant="light"
                          onClick={(e) => {
                            e.stopPropagation();
                            const refund = r.receipt.gross_total < 0;
                            setPaying({ kind: 'receipts', id: r.receipt.id, open: receiptRemaining(r.receipt, r.open_amount), title: `${refund ? 'Erstattung' : 'Zahlung'} für Beleg ${r.receipt.number}`, incoming: receiptIncoming(r.receipt) });
                          }}
                          aria-label="Bezahlen"
                        >
                          <IconCash size={16} />
                        </ActionIcon>
                      </Tooltip>
                    ),
                  },
                ]}
              />
            </Card>
            {(runs ?? []).length > 0 && (
              <Card padding={0}>
                <Text fw={650} p="md" pb="xs">
                  Überweisungsdateien
                </Text>
                <ListTable<PaymentRunView>
                  records={runs}
                  idAccessor={(r) => r.run.id}
                  pageSize={10}
                  minHeight={0}
                  columns={[
                    { accessor: 'run.created_at', title: 'Erstellt', render: (r) => date(r.run.created_at.slice(0, 10)) },
                    { accessor: 'run.execution_date', title: 'Ausführung', render: (r) => date(r.run.execution_date) },
                    { accessor: 'account_name', title: 'Konto' },
                    {
                      accessor: 'items',
                      title: 'Überweisungen',
                      render: (r) => {
                        const paid = r.items.filter((i) => i.paid).length;
                        return (
                          <Tooltip
                            multiline
                            label={r.items.map((i) => (
                              <div key={i.item.id}>
                                {i.receipt_number} · {i.item.creditor_name} · {money(i.item.amount)}
                                {i.paid ? ' · bezahlt' : ''}
                              </div>
                            ))}
                          >
                            <Text size="sm">
                              {r.run.count} Überweisung{r.run.count === 1 ? '' : 'en'} ·{' '}
                              {paid === r.items.length ? (
                                <Text span c="teal.7" size="sm">
                                  alle bezahlt
                                </Text>
                              ) : (
                                `${paid} bezahlt`
                              )}
                            </Text>
                          </Tooltip>
                        );
                      },
                    },
                    {
                      accessor: 'run.total',
                      title: 'Summe',
                      textAlign: 'right',
                      render: (r) => (
                        <Text size="sm" fw={600} className="tabular">
                          {money(r.run.total)}
                        </Text>
                      ),
                    },
                    {
                      accessor: 'actions',
                      title: '',
                      width: 90,
                      render: (r) => (
                        <Group gap={4} wrap="nowrap" justify="flex-end">
                          <Tooltip label="Datei herunterladen">
                            <ActionIcon variant="subtle" onClick={() => downloadFile(`/payment-runs/${r.run.id}/xml`).catch((e) => notifyError(e))} aria-label="Herunterladen">
                              <IconDownload size={16} />
                            </ActionIcon>
                          </Tooltip>
                          <Tooltip label="Verwerfen">
                            <ActionIcon
                              variant="subtle"
                              color="red"
                              aria-label="Verwerfen"
                              onClick={() =>
                                confirmAction({
                                  title: 'Überweisungsdatei verwerfen',
                                  message: 'Nur verwerfen, wenn die Datei nicht an die Bank übergeben wurde. Die Belege bleiben offen und können erneut überwiesen werden.',
                                  confirmLabel: 'Verwerfen',
                                  danger: true,
                                  onConfirm: () => removeRun.mutate(r.run.id),
                                })
                              }
                            >
                              <IconTrash size={16} />
                            </ActionIcon>
                          </Tooltip>
                        </Group>
                      ),
                    },
                  ]}
                />
              </Card>
            )}
          </Stack>
          <TransferModal
            opened={transferOpen}
            candidates={selectedPayables.map((r) => candidateOf.get(r.receipt.id)).filter((c): c is TransferCandidate => !!c)}
            onClose={() => {
              setTransferOpen(false);
              setSelectedPayables([]);
            }}
          />
        </Tabs.Panel>

        <Tabs.Panel value="lastschriften">
          <DebitTab />
        </Tabs.Panel>

        <Tabs.Panel value="mahnwesen">
          <Stack gap="lg">
            <Card padding={0}>
              <Group justify="space-between" p="md">
                <div>
                  <Text fw={650}>Fällige Mahnstufen</Text>
                  <Text size="xs" c="dimmed">
                    Vorschläge nach den Fristen Ihrer Mahnstufen (Einstellungen → Mahnwesen)
                  </Text>
                </div>
                <Button
                  color="orange"
                  leftSection={<IconBellRinging size={16} />}
                  disabled={dueSuggestions.length === 0}
                  loading={runDunning.isPending}
                  onClick={() =>
                    confirmAction({
                      title: 'Mahnlauf für alle fälligen Posten',
                      message: `Es werden ${dueSuggestions.length} Mahnungen erstellt.`,
                      confirmLabel: 'Mahnlauf starten',
                      onConfirm: () => runDunning.mutate(dueSuggestions.map((s) => ({ id: s.document.id, level: s.next_level }))),
                    })
                  }
                >
                  Mahnlauf starten ({dueSuggestions.length})
                </Button>
              </Group>
              <ListTable<DunningSuggestion>
                records={suggestions ?? []}
                idAccessor={(s) => s.document.id}
                defaultSort={{ column: 'document.days_overdue', direction: 'desc' }}
                onRowClick={(s) => navigate(`/dokumente/${s.document.id}`)}
                emptyText="Keine überfälligen Rechnungen"
                columns={[
                  { accessor: 'document.number', title: 'Rechnung', render: (s) => <Text size="sm" fw={600}>{s.document.number}</Text> },
                  { accessor: 'document.contact_name', title: 'Kunde' },
                  { accessor: 'document.days_overdue', title: 'Überfällig', sortable: true, render: (s) => `${s.document.days_overdue} Tage` },
                  { accessor: 'last_dunning', title: 'Letzte Mahnung', render: (s) => (s.last_dunning ? `${s.last_dunning.title} · Frist ${date(s.last_dunning.due_date)}` : '–') },
                  { accessor: 'level_name', title: 'Nächste Stufe', render: (s) => <Badge color={s.due ? 'orange' : 'gray'} variant="light">{s.level_name}{s.due ? ' · fällig' : ''}</Badge> },
                  { accessor: 'fee', title: 'Gebühr', textAlign: 'right', render: (s) => money(s.fee) },
                  { accessor: 'document.open_amount', title: 'Offen', textAlign: 'right', render: (s) => <Text size="sm" fw={600} className="tabular">{money(s.document.open_amount)}</Text> },
                ]}
              />
            </Card>
            <Card padding={0}>
              <Text fw={650} p="md">
                Erstellte Mahnungen
              </Text>
              <ListTable<DunningListItem>
                records={dunnings ?? []}
                idAccessor={(d) => d.dunning.id}
                defaultSort={{ column: 'dunning.date', direction: 'desc' }}
                emptyText="Noch keine Mahnungen erstellt"
                columns={[
                  { accessor: 'dunning.date', title: 'Datum', sortable: true, render: (d) => date(d.dunning.date) },
                  { accessor: 'level_name', title: 'Stufe', render: (d) => <Badge variant="light" color="orange">{d.dunning.title}</Badge> },
                  { accessor: 'document_number', title: 'Rechnung' },
                  { accessor: 'contact_name', title: 'Kunde' },
                  { accessor: 'dunning.due_date', title: 'Frist', render: (d) => date(d.dunning.due_date) },
                  { accessor: 'dunning.fee', title: 'Gebühr', textAlign: 'right', render: (d) => money(d.dunning.fee + d.dunning.interest) },
                  {
                    accessor: 'actions',
                    title: '',
                    width: 80,
                    render: (d) => (
                      <Group gap={2}>
                        <ActionIcon onClick={() => openFile(`/dunning/${d.dunning.id}/pdf`)} aria-label="PDF öffnen">
                          <IconExternalLink size={16} />
                        </ActionIcon>
                        <ActionIcon color="red" onClick={() => confirmAction({ title: 'Mahnung löschen', message: 'Die Mahnung wird gelöscht.', danger: true, confirmLabel: 'Löschen', onConfirm: () => removeDunning.mutate(d.dunning.id) })} aria-label="Löschen">
                          <IconTrash size={14} />
                        </ActionIcon>
                      </Group>
                    ),
                  },
                ]}
              />
            </Card>
          </Stack>
        </Tabs.Panel>
      </Tabs>
      {paying && (
        <PaymentModal opened={!!paying} onClose={() => setPaying(null)} target={paying.kind} id={paying.id} openAmount={paying.open} title={paying.title} incoming={paying.incoming ?? paying.kind === 'documents'} />
      )}
    </Page>
  );
}
