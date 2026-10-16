// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useState } from 'react';
import { Link, useParams, useSearchParams } from 'react-router';
import { Anchor, Badge, Card, SimpleGrid, Stack, Table, Text } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { IconCalendar } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { useApi } from '@/api/hooks';
import type { Ledger } from '@/api/types';
import { KpiCard, Page, PageHeader } from '@/components/ui';
import { date, money } from '@/lib/format';
import { ACCOUNT_KINDS, SOURCE_LABELS } from '@/lib/labels';

const link = (type: string, id: number, cash = false) => (type === 'document' ? `/dokumente/${id}` : type === 'receipt' ? `/belege/${id}` : type === 'transaction' ? (cash ? '/kasse' : '/bank') : type === 'asset' ? '/buchhaltung/anlagen' : null);

const NOWRAP = { whiteSpace: 'nowrap' } as const;

export function LedgerPage() {
  const { number } = useParams();
  const [params] = useSearchParams();
  const [range, setRange] = useState<[string | null, string | null]>([
    params.get('from') || dayjs().startOf('year').format('YYYY-MM-DD'),
    params.get('to') || dayjs().endOf('year').format('YYYY-MM-DD'),
  ]);
  const { data } = useApi<Ledger>(`/ledger/${number}`, { from: range[0] ?? undefined, to: range[1] ?? undefined }, { keepPrevious: true });
  const fmt = (v: number) => `${money(Math.abs(v))} ${v > 0 ? 'S' : v < 0 ? 'H' : ''}`;
  return (
    <Page>
      <PageHeader
        back={{ to: '/buchhaltung/konten', label: 'Kontenplan' }}
        title={`Konto ${number}${data ? ` · ${data.account.name}` : ''}`}
        badge={data && <Badge variant="light">{ACCOUNT_KINDS[data.account.kind] ?? data.account.kind}</Badge>}
        actions={<DatePickerInput type="range" valueFormat="DD.MM.YYYY" value={range} onChange={(v) => setRange(v as [string | null, string | null])} leftSection={<IconCalendar size={16} />} w={250} />}
      />
      <Stack gap="md">
        <SimpleGrid cols={{ base: 2, md: 4 }}>
          <KpiCard label="Anfangssaldo" value={fmt(data?.opening ?? 0)} loading={!data} />
          <KpiCard label="Soll" value={money(data?.debit_total ?? 0)} loading={!data} />
          <KpiCard label="Haben" value={money(data?.credit_total ?? 0)} loading={!data} />
          <KpiCard label="Endsaldo" value={fmt(data?.closing ?? 0)} loading={!data} />
        </SimpleGrid>
        <Card padding={0}>
          <Table.ScrollContainer minWidth={900}>
            <Table verticalSpacing={6} highlightOnHover>
              <Table.Thead bg="var(--app-bg)">
                <Table.Tr>
                  <Table.Th>Datum</Table.Th>
                  <Table.Th>Beleg</Table.Th>
                  <Table.Th miw={240}>Buchungstext</Table.Th>
                  <Table.Th>Gegenkonto</Table.Th>
                  <Table.Th ta="right">Soll</Table.Th>
                  <Table.Th ta="right">Haben</Table.Th>
                  <Table.Th ta="right">Saldo</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                <Table.Tr>
                  <Table.Td colSpan={6}>
                    <Text size="sm" c="dimmed">
                      Saldovortrag
                    </Text>
                  </Table.Td>
                  <Table.Td ta="right" className="tabular" style={NOWRAP}>
                    <Text size="sm">{fmt(data?.opening ?? 0)}</Text>
                  </Table.Td>
                </Table.Tr>
                {(data?.rows ?? []).map((r, i) => {
                  const l = link(r.source_type, r.source_id, data?.account.pick === 'cash');
                  return (
                    <Table.Tr key={i}>
                      <Table.Td style={NOWRAP}>{date(r.date)}</Table.Td>
                      <Table.Td style={NOWRAP}>
                        {l ? (
                          <Anchor component={Link} to={l} size="sm">
                            {r.doc_ref || '–'}
                          </Anchor>
                        ) : (
                          r.doc_ref
                        )}
                      </Table.Td>
                      <Table.Td>
                        <Text size="sm" lineClamp={2}>
                          {r.description}
                        </Text>
                        <Text size="xs" c="dimmed">
                          {SOURCE_LABELS[r.source_type] ?? r.source_type}
                        </Text>
                      </Table.Td>
                      <Table.Td>
                        {r.contra.split(', ').map((c) => (
                          <Anchor key={c} component={Link} to={`/buchhaltung/konten/${c}?from=${range[0]}&to=${range[1]}`} size="sm" mr={6}>
                            {c}
                          </Anchor>
                        ))}
                      </Table.Td>
                      <Table.Td ta="right" className="tabular" style={NOWRAP}>
                        {r.debit ? money(r.debit) : ''}
                      </Table.Td>
                      <Table.Td ta="right" className="tabular" style={NOWRAP}>
                        {r.credit ? money(r.credit) : ''}
                      </Table.Td>
                      <Table.Td ta="right" className="tabular" style={NOWRAP}>
                        <Text size="sm" fw={600}>
                          {fmt(r.balance)}
                        </Text>
                      </Table.Td>
                    </Table.Tr>
                  );
                })}
              </Table.Tbody>
            </Table>
          </Table.ScrollContainer>
          {data && data.rows.length === 0 && (
            <Text size="sm" c="dimmed" ta="center" p="xl">
              Keine Buchungen im Zeitraum
            </Text>
          )}
        </Card>
      </Stack>
    </Page>
  );
}
