// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useState } from 'react';
import { Link } from 'react-router';
import { Alert, Anchor, Badge, Box, Button, Card, Drawer, Group, NumberInput, Select, SimpleGrid, Stack, Table, Text, TextInput, Textarea } from '@mantine/core';
import { IconBuildingWarehouse, IconDownload, IconInfoCircle, IconPlus, IconTrash } from '@tabler/icons-react';
import { api, downloadFile } from '@/api/client';
import { notifyError, useAction, useApi } from '@/api/hooks';
import type { Asset, AssetView } from '@/api/types';
import { AccountSelect, DateField, MoneyInput } from '@/components/inputs';
import { ListTable } from '@/components/ListTable';
import { Empty, KpiCard, Page, PageHeader, confirmAction } from '@/components/ui';
import { date, money, today } from '@/lib/format';
import { ASSET_METHODS } from '@/lib/labels';

function emptyAsset(): Asset {
  return { id: 0, number: '', name: '', account: '', acquisition_date: today(), cost: 0, useful_life_years: 3, method: 'linear', receipt_id: null, receipt_line_id: null, disposal_date: null, disposal_proceeds: 0, notes: '', created_at: '' };
}

const methodLabel = (a: Asset) => (a.method === 'gwg' ? 'GWG' : a.method === 'pool' ? 'Sammelposten' : a.method === 'none' ? 'keine' : `linear, ${a.useful_life_years} J.`);

function AssetDrawer({ opened, onClose, view }: { opened: boolean; onClose: () => void; view: AssetView | null }) {
  const [a, setA] = useState<Asset>(view?.asset ?? emptyAsset());
  useEffect(() => {
    if (opened) setA(view?.asset ?? emptyAsset());
  }, [opened, view]);
  const { data: fresh } = useApi<AssetView>(view ? `/assets/${view.asset.id}` : null);
  const save = useAction((x: Asset) => (x.id ? api.put(`/assets/${x.id}`, x) : api.post('/assets', x)), { success: 'Anlage gespeichert', onSuccess: onClose });
  const remove = useAction(() => api.del(`/assets/${a.id}`), { success: 'Anlage gelöscht', onSuccess: onClose });
  const schedule = fresh?.schedule ?? view?.schedule ?? [];
  return (
    <Drawer opened={opened} onClose={onClose} size="xl" title={<Text fw={650}>{a.id ? `Anlage ${a.number}` : 'Neues Anlagegut'}</Text>}>
      <Stack gap="sm">
        {a.receipt_id && (
          <Alert variant="light" icon={<IconInfoCircle size={16} />} py="xs">
            Aus <Anchor component={Link} to={`/belege/${a.receipt_id}`}>Beleg</Anchor> übernommen. Anschaffungskosten und Datum werden mit dem Beleg synchronisiert.
          </Alert>
        )}
        <TextInput label="Bezeichnung" required value={a.name} onChange={(e) => setA({ ...a, name: e.currentTarget.value })} />
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <AccountSelect label="Anlagekonto" picks={['asset']} value={a.account} onChange={(v) => setA({ ...a, account: v ?? '' })} />
          <TextInput label="Inventarnummer" placeholder="automatisch" value={a.number} onChange={(e) => setA({ ...a, number: e.currentTarget.value })} />
          <DateField label="Anschaffungsdatum" value={a.acquisition_date} onChange={(v) => v && setA({ ...a, acquisition_date: v })} />
          <MoneyInput label="Anschaffungskosten (netto)" value={a.cost} onChange={(v) => setA({ ...a, cost: v })} />
          <Select label="Abschreibungsmethode" data={ASSET_METHODS} value={a.method} onChange={(v) => setA({ ...a, method: v ?? 'linear' })} />
          {a.method === 'linear' && <NumberInput label="Nutzungsdauer (Jahre)" min={1} max={50} value={a.useful_life_years} onChange={(v) => setA({ ...a, useful_life_years: Number(v) || 1 })} description="Laut AfA-Tabelle, z. B. Computer 1–3, Büromöbel 13, Pkw 6" inputWrapperOrder={['label', 'input', 'description', 'error']} />}
          <DateField label="Abgang / Verkauf am" value={a.disposal_date} onChange={(v) => setA({ ...a, disposal_date: v })} clearable />
        </SimpleGrid>
        <Textarea label="Notizen" value={a.notes} onChange={(e) => setA({ ...a, notes: e.currentTarget.value })} />
        {schedule.length > 0 && (
          <Box>
            <Text fw={600} size="sm" mb={4}>
              Abschreibungsplan
            </Text>
            <Table fz="sm" verticalSpacing={4} striped>
              <Table.Thead>
                <Table.Tr>
                  <Table.Th>Jahr</Table.Th>
                  <Table.Th ta="right">Buchwert Anfang</Table.Th>
                  <Table.Th ta="right">AfA</Table.Th>
                  <Table.Th ta="right">Abgang</Table.Th>
                  <Table.Th ta="right">Buchwert Ende</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {schedule.map((r) => (
                  <Table.Tr key={r.year} fw={r.year === new Date().getFullYear() ? 700 : undefined}>
                    <Table.Td>
                      {r.year}
                      {r.months > 0 && r.months < 12 && (
                        <Text span size="xs" c="dimmed" ml={4}>
                          ({r.months} Mon.)
                        </Text>
                      )}
                    </Table.Td>
                    <Table.Td ta="right" className="tabular">{money(r.opening)}</Table.Td>
                    <Table.Td ta="right" className="tabular">{money(r.afa)}</Table.Td>
                    <Table.Td ta="right" className="tabular">{r.disposal ? money(r.disposal) : ''}</Table.Td>
                    <Table.Td ta="right" className="tabular">{money(r.closing)}</Table.Td>
                  </Table.Tr>
                ))}
              </Table.Tbody>
            </Table>
          </Box>
        )}
        <Group justify="space-between" mt="sm">
          {a.id ? (
            <Button variant="subtle" color="red" leftSection={<IconTrash size={16} />} onClick={() => confirmAction({ title: 'Anlage löschen', message: 'Die Anlage und ihre Abschreibungsbuchungen werden gelöscht.', danger: true, confirmLabel: 'Löschen', onConfirm: () => remove.mutate() })}>
              Löschen
            </Button>
          ) : (
            <span />
          )}
          <Group>
            <Button variant="default" onClick={onClose}>
              Abbrechen
            </Button>
            <Button onClick={() => save.mutate(a)} loading={save.isPending}>
              Speichern
            </Button>
          </Group>
        </Group>
      </Stack>
    </Drawer>
  );
}

export function AssetsPage() {
  const { data } = useApi<AssetView[]>('/assets');
  const [drawer, setDrawer] = useState<{ open: boolean; view: AssetView | null }>({ open: false, view: null });
  const year = new Date().getFullYear();
  return (
    <Page>
      <PageHeader
        title="Anlagen & Abschreibungen"
        subtitle="Anlagenverzeichnis mit automatischer AfA (linear, GWG, Sammelposten)"
        actions={
          <>
            <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile('/reports/pdf/assets').catch((e) => notifyError(e))}>
              Anlagenverzeichnis (PDF)
            </Button>
            <Button leftSection={<IconPlus size={16} />} onClick={() => setDrawer({ open: true, view: null })}>
              Anlage erfassen
            </Button>
          </>
        }
      />
      <SimpleGrid cols={{ base: 1, sm: 3 }} mb="lg">
        <KpiCard label="Anlagegüter" value={data?.length ?? 0} loading={!data} icon={<IconBuildingWarehouse size={22} />} />
        <KpiCard label={`AfA ${year}`} value={money((data ?? []).reduce((s, a) => s + a.afa_current_year, 0))} loading={!data} />
        <KpiCard label="Buchwert gesamt" value={money((data ?? []).reduce((s, a) => s + a.book_value, 0))} loading={!data} />
      </SimpleGrid>
      <Card padding={0}>
        {data && data.length === 0 ? (
          <Empty
            icon={<IconBuildingWarehouse size={22} />}
            title="Noch keine Anlagegüter"
            description="Belege auf Anlagekonten (z. B. Büroeinrichtung, GWG) werden automatisch hier übernommen."
          />
        ) : (
          <ListTable<AssetView>
            records={data}
            idAccessor={(a) => a.asset.id}
            defaultSort={{ column: 'asset.acquisition_date', direction: 'desc' }}
            onRowClick={(v) => setDrawer({ open: true, view: v })}
            columns={[
              { accessor: 'asset.number', title: 'Inv.-Nr.', width: 100 },
              { accessor: 'asset.name', title: 'Bezeichnung', sortable: true, render: (v) => <><Text size="sm" fw={600}>{v.asset.name}</Text><Text size="xs" c="dimmed">{v.asset.account} · {v.account_name}</Text></> },
              { accessor: 'asset.acquisition_date', title: 'Anschaffung', sortable: true, render: (v) => date(v.asset.acquisition_date) },
              { accessor: 'asset.cost', title: 'AK netto', textAlign: 'right', sortable: true, render: (v) => <Text size="sm" className="tabular">{money(v.asset.cost)}</Text> },
              { accessor: 'asset.method', title: 'Methode', width: 150, render: (v) => <Badge variant="light" color={v.asset.method === 'gwg' ? 'teal' : v.asset.method === 'pool' ? 'grape' : 'brand'}>{methodLabel(v.asset)}</Badge> },
              { accessor: 'afa_current_year', title: `AfA ${year}`, textAlign: 'right', render: (v) => <Text size="sm" className="tabular">{money(v.afa_current_year)}</Text> },
              { accessor: 'book_value', title: 'Buchwert', textAlign: 'right', sortable: true, render: (v) => <Text size="sm" fw={600} className="tabular">{money(v.book_value)}</Text> },
              { accessor: 'asset.disposal_date', title: '', render: (v) => (v.asset.disposal_date ? <Badge color="gray" size="sm">Abgang {date(v.asset.disposal_date)}</Badge> : null) },
            ]}
          />
        )}
      </Card>
      <AssetDrawer opened={drawer.open} onClose={() => setDrawer({ open: false, view: null })} view={drawer.view} />
    </Page>
  );
}
