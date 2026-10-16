// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { Fragment, useEffect, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router';
import { ActionIcon, Alert, Anchor, Badge, Button, Card, Group, Modal, Select, SimpleGrid, Stack, Table, Text, TextInput, Tooltip } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { IconAlertTriangle, IconCalendar, IconDownload, IconEdit, IconPlus, IconSearch, IconTrash } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api, downloadFile } from '@/api/client';
import { notifyError, useAction, useApi, useChart } from '@/api/hooks';
import type { JournalRow, ManualEntry } from '@/api/types';
import { AccountSelect, DateField, MoneyInput } from '@/components/inputs';
import { Page, PageHeader, confirmAction } from '@/components/ui';
import { date, money, today } from '@/lib/format';
import { SOURCE_LABELS } from '@/lib/labels';

const TAX_KEYS = [
  { value: '', label: 'ohne Steuer' },
  { value: 'VST19', label: 'Vorsteuer 19 %' },
  { value: 'VST7', label: 'Vorsteuer 7 %' },
  { value: 'UST19', label: 'Umsatzsteuer 19 %' },
  { value: 'UST7', label: 'Umsatzsteuer 7 %' },
];

function emptyEntry(): ManualEntry {
  return { id: 0, date: today(), description: '', doc_ref: '', debit_account: '', credit_account: '', amount: 0, tax_key: '', created_at: '' };
}

export function ManualEntryModal({ opened, onClose, entry }: { opened: boolean; onClose: () => void; entry: ManualEntry | null }) {
  const [e, setE] = useState<ManualEntry>(entry ?? emptyEntry());
  useEffect(() => {
    if (opened) setE(entry ?? emptyEntry());
  }, [opened, entry]);
  const save = useAction((m: ManualEntry) => (m.id ? api.put(`/manual-entries/${m.id}`, m) : api.post('/manual-entries', m)), { success: 'Buchung gespeichert', onSuccess: onClose });
  const remove = useAction((id: number) => api.del(`/manual-entries/${id}`), { success: 'Buchung gelöscht', onSuccess: onClose });
  const { data: chart } = useChart();
  const moneyAccount = (chart ?? []).find((a) => (a.number === e.debit_account || a.number === e.credit_account) && (a.pick === 'bank' || a.pick === 'cash'));
  return (
    <Modal opened={opened} onClose={onClose} size="lg" title={<Text fw={650}>{e.id ? 'Buchung bearbeiten' : 'Manuelle Buchung (Soll an Haben)'}</Text>}>
      <Stack gap="sm">
        <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
          <DateField label="Buchungsdatum" value={e.date} onChange={(v) => v && setE({ ...e, date: v })} />
          <TextInput label="Belegnummer" value={e.doc_ref} onChange={(x) => setE({ ...e, doc_ref: x.currentTarget.value })} />
          <MoneyInput label="Betrag (brutto)" value={e.amount} onChange={(v) => setE({ ...e, amount: v })} min={0} />
        </SimpleGrid>
        <TextInput label="Buchungstext" required value={e.description} onChange={(x) => setE({ ...e, description: x.currentTarget.value })} placeholder="z. B. Rückstellung Jahresabschluss" />
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <AccountSelect label="Soll-Konto" placeholder="Konto wählen …" allAccounts value={e.debit_account} onChange={(v) => setE({ ...e, debit_account: v ?? '' })} />
          <AccountSelect label="Haben-Konto" placeholder="Konto wählen …" allAccounts value={e.credit_account} onChange={(v) => setE({ ...e, credit_account: v ?? '' })} />
        </SimpleGrid>
        <Select label="Steuerschlüssel" data={TAX_KEYS} value={e.tax_key} onChange={(v) => setE({ ...e, tax_key: v ?? '' })} description="Vorsteuer wird zum Soll-Konto, Umsatzsteuer zum Haben-Konto herausgerechnet" />
        {moneyAccount && (
          <Alert color="yellow" icon={<IconAlertTriangle size={16} />} p="xs">
            <Text size="sm">
              {moneyAccount.number} {moneyAccount.name} ist ein Zahlungskonto. Zahlungen bitte über {moneyAccount.pick === 'cash' ? 'das Kassenbuch' : 'Bank & Zahlungen'} erfassen – manuelle Buchungen erscheinen dort nicht und der Kontostand weicht sonst vom Sachkonto ab.
            </Text>
          </Alert>
        )}
        <Group justify="space-between" mt="sm">
          {e.id ? (
            <Button variant="subtle" color="red" leftSection={<IconTrash size={16} />} onClick={() => confirmAction({ title: 'Buchung löschen', message: `Die manuelle Buchung „${e.description}“ wird gelöscht.`, danger: true, confirmLabel: 'Löschen', onConfirm: () => remove.mutate(e.id) })}>
              Löschen
            </Button>
          ) : (
            <span />
          )}
          <Group>
            <Button variant="default" onClick={onClose}>
              Abbrechen
            </Button>
            <Button onClick={() => save.mutate(e)} loading={save.isPending} disabled={!e.debit_account || !e.credit_account || e.amount <= 0 || !e.description}>
              Buchen
            </Button>
          </Group>
        </Group>
      </Stack>
    </Modal>
  );
}

const sourceLink = (type: string, id: number, cash: boolean) => {
  switch (type) {
    case 'document':
      return `/dokumente/${id}`;
    case 'receipt':
      return `/belege/${id}`;
    case 'asset':
      return '/buchhaltung/anlagen';
    case 'transaction':
      return cash ? '/kasse' : '/bank';
    default:
      return null;
  }
};

export function JournalPage() {
  const [params, setParams] = useSearchParams();
  const [range, setRange] = useState<[string | null, string | null]>([dayjs().startOf('year').format('YYYY-MM-DD'), dayjs().endOf('year').format('YYYY-MM-DD')]);
  const [account, setAccount] = useState<string | null>(null);
  const [q, setQ] = useState('');
  const [modal, setModal] = useState<{ open: boolean; entry: ManualEntry | null }>({ open: false, entry: null });
  const { data: chart } = useChart();
  const { data, isFetching } = useApi<JournalRow[]>('/journal', { from: range[0] ?? undefined, to: range[1] ?? undefined, account: account ?? undefined, q: q || undefined }, { keepPrevious: true });
  const { data: manual } = useApi<ManualEntry[]>('/manual-entries');
  const names = useMemo(() => new Map((chart ?? []).map((a) => [a.number, a.name])), [chart]);
  const cashAccounts = useMemo(() => new Set((chart ?? []).filter((a) => a.pick === 'cash').map((a) => a.number)), [chart]);
  const [limit, setLimit] = useState(150);

  useEffect(() => {
    if (params.get('neu') === '1') {
      setModal({ open: true, entry: null });
      setParams({}, { replace: true });
    }
  }, [params, setParams]);

  const total = (data ?? []).reduce((s, r) => s + r.amount, 0);

  return (
    <Page>
      <PageHeader
        title="Buchungsjournal"
        subtitle="Alle Buchungssätze in zeitlicher Reihenfolge – automatisch erzeugt aus Rechnungen, Belegen, Zahlungen und Abschreibungen"
        actions={
          <>
            <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile('/reports/pdf/journal', { from: range[0] ?? undefined, to: range[1] ?? undefined }).catch((e) => notifyError(e))}>
              PDF
            </Button>
            <Button leftSection={<IconPlus size={16} />} onClick={() => setModal({ open: true, entry: null })}>
              Manuelle Buchung
            </Button>
          </>
        }
      />
      <Stack gap="md">
        <Group>
          <DatePickerInput type="range" valueFormat="DD.MM.YYYY" value={range} onChange={(v) => setRange(v as [string | null, string | null])} leftSection={<IconCalendar size={16} />} w={250} />
          <AccountSelect allAccounts placeholder="Alle Konten" value={account} onChange={setAccount} clearable allowDeselect w={300} />
          <TextInput placeholder="Text oder Beleg …" leftSection={<IconSearch size={16} />} value={q} onChange={(e) => setQ(e.currentTarget.value)} w={220} />
          <Text size="sm" c="dimmed" ml="auto">
            {data?.length ?? 0} Buchungen · Buchungsvolumen {money(total)}
            {isFetching ? ' …' : ''}
          </Text>
        </Group>
        <Card padding={0}>
          <Table.ScrollContainer minWidth={900}>
            <Table verticalSpacing={6} horizontalSpacing="md">
              <Table.Thead bg="var(--app-bg)">
                <Table.Tr>
                  <Table.Th w={100}>Datum</Table.Th>
                  <Table.Th w={130}>Beleg</Table.Th>
                  <Table.Th miw={240}>Buchungstext</Table.Th>
                  <Table.Th w={70}>Konto</Table.Th>
                  <Table.Th>Kontobezeichnung</Table.Th>
                  <Table.Th ta="right" w={120}>
                    Soll
                  </Table.Th>
                  <Table.Th ta="right" w={120}>
                    Haben
                  </Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {(data ?? []).slice(0, limit).map((r) => {
                  const link = sourceLink(r.entry.source_type, r.entry.source_id, r.lines.some((l) => cashAccounts.has(l.account)));
                  const m = r.entry.source_type === 'manual' ? manual?.find((x) => x.id === r.entry.source_id) : null;
                  const span = r.lines.length;
                  return (
                    <Fragment key={r.entry.id}>
                      {r.lines.map((l, i) => (
                        <Table.Tr key={l.id} style={{ borderTop: i === 0 ? '2px solid var(--app-border)' : undefined }}>
                          {i === 0 && (
                            <>
                              <Table.Td rowSpan={span} valign="top">
                                <Text size="sm">{date(r.entry.date)}</Text>
                              </Table.Td>
                              <Table.Td rowSpan={span} valign="top">
                                {link ? (
                                  <Anchor component={Link} to={link} size="sm" style={{ whiteSpace: 'nowrap' }}>
                                    {r.entry.doc_ref || '–'}
                                  </Anchor>
                                ) : (
                                  <Text size="sm" style={{ whiteSpace: 'nowrap' }}>
                                    {r.entry.doc_ref || '–'}
                                  </Text>
                                )}
                              </Table.Td>
                              <Table.Td rowSpan={span} valign="top">
                                <Group gap={6} wrap="nowrap" align="flex-start" justify="space-between">
                                  <div style={{ minWidth: 0 }}>
                                    <Text size="sm" lineClamp={2}>
                                      {r.entry.description}
                                    </Text>
                                    <Text size="xs" c="dimmed">
                                      {SOURCE_LABELS[r.entry.source_type] ?? r.entry.source_type}
                                    </Text>
                                  </div>
                                  {m && (
                                    <Tooltip label="Buchung bearbeiten">
                                      <ActionIcon size="sm" variant="subtle" onClick={() => setModal({ open: true, entry: m })} aria-label="Buchung bearbeiten">
                                        <IconEdit size={14} />
                                      </ActionIcon>
                                    </Tooltip>
                                  )}
                                </Group>
                              </Table.Td>
                            </>
                          )}
                          <Table.Td>
                            <Anchor component={Link} to={`/buchhaltung/konten/${l.account}?from=${range[0] ?? ''}&to=${range[1] ?? ''}`} size="sm" fw={600}>
                              {l.account}
                            </Anchor>
                          </Table.Td>
                          <Table.Td>
                            <Text size="sm" c="dimmed" lineClamp={1} title={names.get(l.account)}>
                              {names.get(l.account)}
                              {l.tax_code && (
                                <Badge size="xs" variant="outline" ml={6} color="violet">
                                  {l.tax_code}
                                </Badge>
                              )}
                            </Text>
                          </Table.Td>
                          <Table.Td ta="right" className="tabular">
                            <Text size="sm">{l.debit ? money(l.debit) : ''}</Text>
                          </Table.Td>
                          <Table.Td ta="right" className="tabular">
                            <Text size="sm">{l.credit ? money(l.credit) : ''}</Text>
                          </Table.Td>
                        </Table.Tr>
                      ))}
                    </Fragment>
                  );
                })}
              </Table.Tbody>
            </Table>
          </Table.ScrollContainer>
          {(data?.length ?? 0) > limit && (
            <Group justify="center" p="md">
              <Button variant="subtle" onClick={() => setLimit((l) => l + 200)}>
                Weitere Buchungen laden ({(data?.length ?? 0) - limit})
              </Button>
            </Group>
          )}
          {data && data.length === 0 && (
            <Text size="sm" c="dimmed" ta="center" p="xl">
              Keine Buchungen im Zeitraum
            </Text>
          )}
        </Card>
      </Stack>
      <ManualEntryModal opened={modal.open} onClose={() => setModal({ open: false, entry: null })} entry={modal.entry} />
    </Page>
  );
}
