import { useMemo, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router';
import { ActionIcon, Badge, Button, Card, Group, Paper, SimpleGrid, Stack, Tabs, Text, Tooltip, UnstyledButton } from '@mantine/core';
import { IconBellRinging, IconCash, IconDownload, IconExternalLink, IconFileExport, IconTrash } from '@tabler/icons-react';
import { api, downloadFile, openFile } from '@/api/client';
import { notifyError, useAction, useApi } from '@/api/hooks';
import type { AgingBucket, DocumentListItem, DunningListItem, DunningSuggestion, OpenItems, ReceiptListItem } from '@/api/types';
import { ListTable } from '@/components/ListTable';
import { PaymentModal } from '@/components/PaymentModal';
import { Page, PageHeader, StatusBadge, confirmAction } from '@/components/ui';
import { exportCsv } from '@/lib/csv';
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
  const [paying, setPaying] = useState<{ kind: 'documents' | 'receipts'; id: number; open: number; title: string; fees?: number } | null>(null);

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
                        <ActionIcon color="teal" variant="light" onClick={(e) => { e.stopPropagation(); setPaying({ kind: 'documents', id: d.id, open: d.open_amount, title: `Zahlung für ${d.number}` }); }} aria-label="Zahlung erfassen">
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
              <ListTable<ReceiptListItem>
                records={payables}
                idAccessor={(r) => r.receipt.id}
                defaultSort={{ column: 'receipt.due_date', direction: 'asc' }}
                onRowClick={(r) => navigate(`/belege/${r.receipt.id}`)}
                emptyText="Keine offenen Verbindlichkeiten"
                columns={[
                  { accessor: 'receipt.number', title: 'Beleg', render: (r) => <><Text size="sm" fw={600}>{r.receipt.number}</Text><Text size="xs" c="dimmed">{r.receipt.invoice_number}</Text></> },
                  { accessor: 'party', title: 'Lieferant', sortable: true },
                  { accessor: 'receipt.date', title: 'Datum', sortable: true, render: (r) => date(r.receipt.date) },
                  { accessor: 'receipt.due_date', title: 'Fällig', sortable: true, render: (r) => <Text size="sm" c={r.overdue ? 'red.7' : undefined}>{date(r.receipt.due_date)}</Text> },
                  { accessor: 'receipt.status', title: 'Status', render: (r) => <StatusBadge status={r.receipt.status} docType="invoice" daysOverdue={r.overdue ? 1 : 0} size="sm" /> },
                  { accessor: 'open_amount', title: 'Offen', sortable: true, textAlign: 'right', render: (r) => <Text size="sm" fw={600} className="tabular">{money(r.open_amount)}</Text> },
                  {
                    accessor: 'pay',
                    title: '',
                    width: 60,
                    render: (r) => (
                      <Tooltip label="Als bezahlt erfassen">
                        <ActionIcon color="teal" variant="light" onClick={(e) => { e.stopPropagation(); setPaying({ kind: 'receipts', id: r.receipt.id, open: r.open_amount, title: `Zahlung für Beleg ${r.receipt.number}` }); }} aria-label="Bezahlen">
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
        <PaymentModal opened={!!paying} onClose={() => setPaying(null)} target={paying.kind} id={paying.id} openAmount={paying.open} title={paying.title} incoming={paying.kind === 'documents'} />
      )}
    </Page>
  );
}
