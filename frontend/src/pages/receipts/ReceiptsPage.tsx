// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router';
import { Button, Card, Group, Loader, SegmentedControl, Stack, Text, TextInput, ThemeIcon, Tooltip } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { Dropzone } from '@mantine/dropzone';
import { IconCalendar, IconCloudUpload, IconFileCode, IconFileExport, IconPaperclip, IconPlus, IconReceipt2, IconSearch } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import { notifyError, useApi } from '@/api/hooks';
import type { ImportResult, ReceiptListItem } from '@/api/types';
import { ListTable } from '@/components/ListTable';
import { Empty, Page, PageHeader, StatPill, StatusBadge } from '@/components/ui';
import { exportCsv } from '@/lib/csv';
import { date, money } from '@/lib/format';

export const RECEIPT_ACCEPT = ['application/pdf', 'image/png', 'image/jpeg', 'image/webp', 'image/heic', 'application/xml', 'text/xml'];

export function useReceiptUpload() {
  const navigate = useNavigate();
  const [uploading, setUploading] = useState(false);
  const upload = async (file: File) => {
    setUploading(true);
    try {
      const result = await api.upload<ImportResult>('/receipts/import', file);
      navigate('/belege/neu', { state: { import: result } });
    } catch (e) {
      notifyError(e, 'Upload fehlgeschlagen');
    } finally {
      setUploading(false);
    }
  };
  return { upload, uploading };
}

export function ReceiptsPage() {
  const navigate = useNavigate();
  const [status, setStatus] = useState('all');
  const [type, setType] = useState('expense');
  const [q, setQ] = useState('');
  const [range, setRange] = useState<[string | null, string | null]>([null, null]);
  const { data, isFetching } = useApi<ReceiptListItem[]>(
    '/receipts',
    { status: status === 'all' ? undefined : status, receipt_type: type === 'all' ? undefined : type, from: range[0] ?? undefined, to: range[1] ?? undefined },
    { keepPrevious: true },
  );
  const { upload, uploading } = useReceiptUpload();

  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    return (data ?? []).filter((r) => !s || `${r.receipt.number} ${r.receipt.invoice_number} ${r.party} ${r.receipt.description} ${r.categories.join(' ')}`.toLowerCase().includes(s));
  }, [data, q]);
  const sums = useMemo(
    () => ({ gross: filtered.reduce((a, r) => a + r.receipt.gross_total, 0), net: filtered.reduce((a, r) => a + r.receipt.net_total, 0), open: filtered.reduce((a, r) => a + r.open_amount, 0) }),
    [filtered],
  );

  return (
    <Page>
      <PageHeader
        title="Belege"
        subtitle="Eingangsrechnungen, Quittungen und E-Rechnungen erfassen und kategorisieren"
        actions={
          <>
            <Button
              variant="default"
              leftSection={<IconFileExport size={16} />}
              onClick={() =>
                exportCsv(`Belege_${dayjs().format('YYYY-MM-DD')}`, filtered, [
                  { label: 'Belegnr.', value: (r) => r.receipt.number },
                  { label: 'Rechnungsnr.', value: (r) => r.receipt.invoice_number },
                  { label: 'Datum', value: (r) => date(r.receipt.date) },
                  { label: 'Geschäftspartner', value: (r) => r.party },
                  { label: 'Kategorie', value: (r) => r.categories.join(', ') },
                  { label: 'Netto', value: (r) => r.receipt.net_total },
                  { label: 'USt', value: (r) => r.receipt.tax_total },
                  { label: 'Brutto', value: (r) => r.receipt.gross_total },
                  { label: 'Offen', value: (r) => r.open_amount },
                ])
              }
            >
              Export
            </Button>
            <Button leftSection={<IconPlus size={16} />} onClick={() => navigate('/belege/neu')}>
              Beleg erfassen
            </Button>
          </>
        }
      />
      <Stack gap="md">
        <Dropzone onDrop={(files) => files[0] && upload(files[0])} accept={RECEIPT_ACCEPT} maxFiles={1} loading={uploading} radius="lg" p="md">
          <Group justify="center" gap="md" mih={64} style={{ pointerEvents: 'none' }}>
            <Dropzone.Accept>
              <IconCloudUpload size={32} color="var(--mantine-color-brand-6)" />
            </Dropzone.Accept>
            <Dropzone.Reject>
              <IconCloudUpload size={32} color="var(--mantine-color-red-6)" />
            </Dropzone.Reject>
            <Dropzone.Idle>
              <ThemeIcon size={44} radius="xl" variant="light">
                <IconCloudUpload size={24} />
              </ThemeIcon>
            </Dropzone.Idle>
            <div>
              <Text fw={600}>Beleg hierher ziehen oder klicken</Text>
              <Text size="sm" c="dimmed">
                PDF, Foto, XRechnung oder ZUGFeRD – E-Rechnungen werden automatisch ausgelesen
              </Text>
            </div>
            {uploading && <Loader size="sm" />}
          </Group>
        </Dropzone>

        <Group justify="space-between" wrap="wrap">
          <Group gap="sm">
            <SegmentedControl
              value={type}
              onChange={setType}
              data={[
                { value: 'expense', label: 'Ausgaben' },
                { value: 'income', label: 'Einnahmen' },
                { value: 'all', label: 'Alle' },
              ]}
            />
            <SegmentedControl
              value={status}
              onChange={setStatus}
              data={[
                { value: 'all', label: 'Alle' },
                { value: 'unpaid', label: 'Unbezahlt' },
                { value: 'overdue', label: 'Überfällig' },
                { value: 'paid', label: 'Bezahlt' },
              ]}
            />
          </Group>
          <Group gap="sm">
            <DatePickerInput type="range" placeholder="Zeitraum" valueFormat="DD.MM.YYYY" value={range} onChange={(v) => setRange(v as [string | null, string | null])} clearable leftSection={<IconCalendar size={16} />} w={240} />
            <TextInput placeholder="Suchen …" leftSection={<IconSearch size={16} />} value={q} onChange={(e) => setQ(e.currentTarget.value)} w={220} />
          </Group>
        </Group>
        {filtered.length > 0 && (
          <Group gap="sm">
            <StatPill label={`${filtered.length} Belege · netto`} value={money(sums.net)} />
            <StatPill label="Brutto" value={money(sums.gross)} />
            <StatPill label="Davon offen" value={money(sums.open)} color={sums.open > 0 ? 'orange.8' : undefined} />
          </Group>
        )}
        <Card padding={0}>
          {data && data.length === 0 && status === 'all' && !q ? (
            <Empty icon={<IconReceipt2 size={22} />} title="Noch keine Belege" description="Laden Sie Ihren ersten Beleg hoch oder erfassen Sie ihn manuell." />
          ) : (
            <ListTable<ReceiptListItem>
              records={filtered}
              idAccessor={(r) => r.receipt.id}
              fetching={isFetching}
              defaultSort={{ column: 'receipt.date', direction: 'desc' }}
              onRowClick={(r) => navigate(`/belege/${r.receipt.id}`)}
              columns={[
                {
                  accessor: 'receipt.number',
                  title: 'Beleg',
                  sortable: true,
                  render: (r) => (
                    <Stack gap={0}>
                      <Group gap={6} wrap="nowrap">
                        <Text size="sm" fw={600}>
                          {r.receipt.number}
                        </Text>
                        {r.file_name && (
                          <Tooltip label={r.file_name}>
                            <IconPaperclip size={14} color="var(--app-muted)" />
                          </Tooltip>
                        )}
                        {r.receipt.source !== 'manual' && (
                          <Tooltip label={r.receipt.source === 'zugferd' ? 'ZUGFeRD-Rechnung' : 'XRechnung'}>
                            <IconFileCode size={14} color="var(--mantine-color-violet-6)" />
                          </Tooltip>
                        )}
                      </Group>
                      {r.receipt.invoice_number && (
                        <Text size="xs" c="dimmed">
                          Re.-Nr. {r.receipt.invoice_number}
                        </Text>
                      )}
                    </Stack>
                  ),
                },
                { accessor: 'receipt.date', title: 'Datum', sortable: true, render: (r) => date(r.receipt.date), width: 110 },
                { accessor: 'party', title: 'Geschäftspartner', sortable: true, ellipsis: true },
                {
                  accessor: 'categories',
                  title: 'Kategorie',
                  visibleMediaQuery: '(min-width: 62em)',
                  render: (r) => (
                    <Text size="sm" c="dimmed" lineClamp={2} maw={320}>
                      {r.categories.join(" · ")}
                    </Text>
                  ),
                },
                { accessor: 'receipt.gross_total', title: 'Betrag', sortable: true, textAlign: 'right', render: (r) => <Text size="sm" fw={600} className="tabular">{money(r.receipt.gross_total)}</Text> },
                {
                  accessor: 'receipt.status',
                  title: 'Status',
                  sortable: true,
                  width: 130,
                  render: (r) => <StatusBadge status={r.receipt.status === 'open' ? 'open' : r.receipt.status} docType="invoice" daysOverdue={r.overdue ? 1 : 0} size="sm" />,
                },
              ]}
            />
          )}
        </Card>
      </Stack>
    </Page>
  );
}
