// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useMemo, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router';
import { ActionIcon, Badge, Button, Card, Group, Menu, SegmentedControl, Stack, Text, TextInput, Tooltip, ScrollArea } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import {
  IconCalendar,
  IconCopy,
  IconDots,
  IconDownload,
  IconExternalLink,
  IconFileCode,
  IconFileExport,
  IconRepeat,
  IconPlus,
  IconSearch,
  IconTrash,
  IconFileInvoice,
  IconBellRinging,
} from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api, downloadFile, openFile } from '@/api/client';
import { notifyError, useAction, useApi } from '@/api/hooks';
import type { DocumentListItem, IdResponse } from '@/api/types';
import { ListTable } from '@/components/ListTable';
import { RecurringDrawer } from '@/components/Recurring';
import { Empty, Page, PageHeader, StatPill, StatusBadge, confirmAction } from '@/components/ui';
import { exportCsv } from '@/lib/csv';
import { date, money, relativeDays } from '@/lib/format';
import { DOC_TYPES, docLabel, statusInfo, type DocType } from '@/lib/labels';

const STATUS_TABS: Record<DocType, { value: string; label: string }[]> = {
  invoice: [
    { value: 'all', label: 'Alle' },
    { value: 'draft', label: 'Entwürfe' },
    { value: 'unpaid', label: 'Offen' },
    { value: 'overdue', label: 'Überfällig' },
    { value: 'paid', label: 'Bezahlt' },
    { value: 'cancelled', label: 'Storniert' },
  ],
  credit_note: [
    { value: 'all', label: 'Alle' },
    { value: 'draft', label: 'Entwürfe' },
    { value: 'unpaid', label: 'Offen' },
    { value: 'paid', label: 'Erstattet' },
  ],
  quote: [
    { value: 'all', label: 'Alle' },
    { value: 'draft', label: 'Entwürfe' },
    { value: 'open', label: 'Versendet' },
    { value: 'accepted', label: 'Angenommen' },
    { value: 'rejected', label: 'Abgelehnt' },
  ],
  order: [
    { value: 'all', label: 'Alle' },
    { value: 'draft', label: 'Entwürfe' },
    { value: 'open', label: 'Offen' },
    { value: 'done', label: 'Erledigt' },
  ],
  delivery: [
    { value: 'all', label: 'Alle' },
    { value: 'draft', label: 'Entwürfe' },
    { value: 'open', label: 'Offen' },
    { value: 'done', label: 'Geliefert' },
  ],
};

export function DocumentsPage({ docType }: { docType: DocType }) {
  const meta = DOC_TYPES[docType];
  const navigate = useNavigate();
  const remove = useAction((id: number) => api.del(`/documents/${id}`), { success: 'Dokument gelöscht' });
  const duplicate = useAction((id: number) => api.post<IdResponse>(`/documents/${id}/copy`), { success: 'Kopie erstellt', onSuccess: (r) => navigate(`/dokumente/${r.id}/bearbeiten`) });
  const [params, setParams] = useSearchParams();
  const status = params.get('status') ?? 'all';
  const [q, setQ] = useState('');
  const [range, setRange] = useState<[string | null, string | null]>([null, null]);
  const [recurringOpen, setRecurringOpen] = useState(false);
  const { data, isFetching } = useApi<DocumentListItem[]>('/documents', {
    doc_type: docType,
    status: status === 'all' ? undefined : status,
    from: range[0] ?? undefined,
    to: range[1] ?? undefined,
  }, { keepPrevious: true });
  const { data: counts } = useApi<Record<string, number>>('/documents/counts', { doc_type: docType });
  const isInvoiceLike = docType === 'invoice' || docType === 'credit_note';

  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    if (!s) return data ?? [];
    return (data ?? []).filter((d) => `${d.number} ${d.contact_name} ${d.title}`.toLowerCase().includes(s));
  }, [data, q]);

  const sums = useMemo(() => {
    const sign = (d: DocumentListItem) => (d.status === 'draft' ? 0 : 1);
    return {
      net: filtered.reduce((a, d) => a + d.net_total * sign(d), 0),
      gross: filtered.reduce((a, d) => a + d.gross_total * sign(d), 0),
      open: filtered.reduce((a, d) => a + d.open_amount, 0),
    };
  }, [filtered]);

  const download = (d: DocumentListItem) => downloadFile(`/documents/${d.id}/pdf`).catch((e) => notifyError(e));
  const tabs = STATUS_TABS[docType].map((t) => ({
    value: t.value,
    label: (
      <Group gap={6} wrap="nowrap" justify="center">
        {t.label}
        {counts?.[t.value] ? (
          <Badge size="xs" variant={t.value === status ? 'filled' : 'light'} color={t.value === 'overdue' ? 'red' : 'gray'} circle={(counts?.[t.value] ?? 0) < 10}>
            {counts?.[t.value]}
          </Badge>
        ) : null}
      </Group>
    ),
  }));

  return (
    <Page>
      <PageHeader
        title={meta.plural}
        subtitle={
          docType === 'invoice'
            ? 'Rechnungen schreiben, als PDF oder E-Rechnung versenden und Zahlungseingänge verfolgen'
            : docType === 'quote'
              ? 'Angebote erstellen und in Aufträge oder Rechnungen umwandeln'
              : docType === 'order'
                ? 'Aufträge bestätigen, Abschlags- und Schlussrechnungen erstellen'
                : docType === 'delivery'
                  ? 'Lieferscheine für Warenlieferungen'
                  : 'Gutschriften und Rechnungskorrekturen'
        }
        actions={
          <>
            {docType === 'invoice' && (
              <Button variant="default" leftSection={<IconRepeat size={16} />} onClick={() => setRecurringOpen(true)}>
                Abo-Rechnungen
              </Button>
            )}
            <Button
              variant="default"
              leftSection={<IconFileExport size={16} />}
              onClick={() =>
                exportCsv(`${meta.plural}_${dayjs().format('YYYY-MM-DD')}`, filtered, [
                  { label: 'Nummer', value: (d) => d.number },
                  { label: 'Art', value: (d) => docLabel(d.doc_type, d.kind) },
                  { label: 'Kunde', value: (d) => d.contact_name },
                  { label: 'Datum', value: (d) => date(d.date) },
                  { label: 'Fällig', value: (d) => date(d.due_date) },
                  { label: 'Netto', value: (d) => d.net_total },
                  { label: 'Brutto', value: (d) => d.gross_total },
                  { label: 'Offen', value: (d) => d.open_amount },
                  { label: 'Status', value: (d) => statusInfo(d.status, d.doc_type, d.days_overdue).label },
                ])
              }
            >
              Export
            </Button>
            <Button leftSection={<IconPlus size={16} />} onClick={() => navigate(`/dokumente/neu?type=${docType}`)}>
              {meta.newLabel}
            </Button>
          </>
        }
      />

      <Stack gap="md">
        <Group justify="space-between" wrap="wrap" gap="sm">
          <ScrollArea type="never" maw="100%">
            <SegmentedControl value={status} onChange={(v) => setParams(v === 'all' ? {} : { status: v })} data={tabs} />
          </ScrollArea>
          <Group gap="sm">
            <DatePickerInput
              type="range"
              placeholder="Zeitraum"
              valueFormat="DD.MM.YYYY"
              value={range}
              onChange={(v) => setRange(v as [string | null, string | null])}
              clearable
              leftSection={<IconCalendar size={16} />}
              w={240}
            />
            <TextInput placeholder="Suchen …" leftSection={<IconSearch size={16} />} value={q} onChange={(e) => setQ(e.currentTarget.value)} w={220} />
          </Group>
        </Group>

        {filtered.length > 0 && (
          <Group gap="sm">
            <StatPill label={`${filtered.length} Dokumente · netto`} value={money(sums.net)} />
            <StatPill label="Brutto" value={money(sums.gross)} />
            {isInvoiceLike && <StatPill label="Davon offen" value={money(sums.open)} color={sums.open > 0 ? 'orange.8' : undefined} />}
          </Group>
        )}

        <Card padding={0}>
          {data && data.length === 0 && status === 'all' && !q ? (
            <Empty
              icon={<IconFileInvoice size={22} />}
              title={`Noch keine ${meta.plural}`}
              description={`Erstellen Sie Ihr erstes Dokument – in wenigen Sekunden fertig als PDF.`}
              action={
                <Button leftSection={<IconPlus size={16} />} onClick={() => navigate(`/dokumente/neu?type=${docType}`)}>
                  {meta.newLabel}
                </Button>
              }
            />
          ) : (
            <ListTable<DocumentListItem>
              records={filtered}
              fetching={isFetching}
              defaultSort={{ column: 'date', direction: 'desc' }}
              onRowClick={(d) => navigate(d.status === 'draft' ? `/dokumente/${d.id}/bearbeiten` : `/dokumente/${d.id}`)}
              columns={[
                {
                  accessor: 'number',
                  title: 'Nummer',
                  sortable: true,
                  render: (d) => (
                    <Stack gap={0}>
                      <Text size="sm" fw={600}>
                        {d.number || <Text span c="dimmed" size="sm">Entwurf</Text>}
                      </Text>
                      {d.kind !== 'standard' && (
                        <Text size="xs" c="dimmed">
                          {docLabel(d.doc_type, d.kind)}
                        </Text>
                      )}
                    </Stack>
                  ),
                },
                { accessor: 'contact_name', title: 'Kunde', sortable: true, ellipsis: true },
                { accessor: 'date', title: 'Datum', sortable: true, render: (d) => date(d.date), width: 110 },
                ...(isInvoiceLike
                  ? [
                      {
                        accessor: 'due_date',
                        title: 'Fällig',
                        sortable: true,
                        width: 150,
                        visibleMediaQuery: '(min-width: 62em)',
                        render: (d: DocumentListItem) =>
                          d.due_date && d.open_amount > 0 && d.status !== 'draft' ? (
                            <Text size="sm" c={d.days_overdue > 0 ? 'red.7' : undefined}>
                              {date(d.due_date)}
                              <Text span size="xs" c="dimmed" ml={6}>
                                {relativeDays(d.due_date)}
                              </Text>
                            </Text>
                          ) : (
                            <Text size="sm" c="dimmed">
                              {date(d.due_date)}
                            </Text>
                          ),
                      },
                    ]
                  : docType === 'quote'
                    ? [{ accessor: 'valid_until', title: 'Gültig bis', sortable: true, render: (d: DocumentListItem) => date(d.valid_until), width: 110, visibleMediaQuery: '(min-width: 62em)' }]
                    : []),
                { accessor: 'gross_total', title: 'Betrag', sortable: true, textAlign: 'right', render: (d) => <Text size="sm" fw={600} className="tabular">{money(d.gross_total)}</Text> },
                ...(isInvoiceLike
                  ? [
                      {
                        accessor: 'open_amount',
                        title: 'Offen',
                        sortable: true,
                        textAlign: 'right' as const,
                        visibleMediaQuery: '(min-width: 48em)',
                        render: (d: DocumentListItem) =>
                          d.open_amount > 0 ? (
                            <Text size="sm" className="tabular" c={d.days_overdue > 0 ? 'red.7' : undefined}>
                              {money(d.open_amount)}
                            </Text>
                          ) : (
                            <Text size="sm" c="dimmed">
                              –
                            </Text>
                          ),
                      },
                    ]
                  : []),
                {
                  accessor: 'status',
                  title: 'Status',
                  sortable: true,
                  width: 150,
                  render: (d) => (
                    <Group gap={4} wrap="nowrap">
                      <StatusBadge status={d.status} docType={d.doc_type} daysOverdue={d.days_overdue} size="sm" />
                      {d.dunning_count > 0 && (
                        <Tooltip label={`${d.dunning_count} Mahnung(en)`}>
                          <Badge size="sm" color="orange" variant="light" leftSection={<IconBellRinging size={10} />}>
                            {d.dunning_count}
                          </Badge>
                        </Tooltip>
                      )}
                      {d.einvoice_format !== 'pdf' && d.einvoice_format && isInvoiceLike && (
                        <Tooltip label={d.einvoice_format === 'zugferd' ? 'ZUGFeRD' : 'XRechnung'}>
                          <IconFileCode size={14} color="var(--mantine-color-violet-6)" />
                        </Tooltip>
                      )}
                    </Group>
                  ),
                },
                {
                  accessor: 'actions',
                  title: '',
                  width: 80,
                  textAlign: 'right',
                  render: (d) => (
                    <Group gap={2} justify="flex-end" wrap="nowrap" onClick={(e) => e.stopPropagation()}>
                      <Tooltip label="PDF herunterladen">
                        <ActionIcon onClick={() => download(d)} aria-label="PDF herunterladen">
                          <IconDownload size={16} />
                        </ActionIcon>
                      </Tooltip>
                      <Menu position="bottom-end">
                        <Menu.Target>
                          <ActionIcon aria-label="Mehr">
                            <IconDots size={16} />
                          </ActionIcon>
                        </Menu.Target>
                        <Menu.Dropdown>
                          <Menu.Item leftSection={<IconExternalLink size={14} />} onClick={() => openFile(`/documents/${d.id}/pdf`)}>
                            PDF öffnen
                          </Menu.Item>
                          <Menu.Item onClick={() => navigate(`/dokumente/${d.id}`)}>Details anzeigen</Menu.Item>
                          <Menu.Item leftSection={<IconCopy size={14} />} onClick={() => duplicate.mutate(d.id)}>
                            Duplizieren
                          </Menu.Item>
                          {!isInvoiceLike && (
                            <>
                              <Menu.Divider />
                              <Menu.Item
                                leftSection={<IconTrash size={14} />}
                                color="red"
                                onClick={() =>
                                  confirmAction({
                                    title: 'Dokument löschen',
                                    message: `${docLabel(d.doc_type, d.kind)} ${d.number || d.title} wirklich löschen?`,
                                    confirmLabel: 'Löschen',
                                    danger: true,
                                    onConfirm: () => remove.mutate(d.id),
                                  })
                                }
                              >
                                Löschen
                              </Menu.Item>
                            </>
                          )}
                          {d.contact_id && <Menu.Item onClick={() => navigate(`/kontakte/${d.contact_id}`)}>Kontakt öffnen</Menu.Item>}
                        </Menu.Dropdown>
                      </Menu>
                    </Group>
                  ),
                },
              ]}
            />
          )}
        </Card>
      </Stack>
      {docType === 'invoice' && <RecurringDrawer opened={recurringOpen} onClose={() => setRecurringOpen(false)} />}
    </Page>
  );
}
