// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// SEPA-Lastschriften: invoices to collect, direct debit files (pain.008) and their history.
import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router';
import { ActionIcon, Alert, Badge, Button, Card, Group, Modal, Select, Stack, Table, Text, Tooltip } from '@mantine/core';
import { IconAlertTriangle, IconBuildingBank, IconDownload, IconFileDownload, IconInfoCircle, IconTrash } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api, downloadFile } from '@/api/client';
import { notifyError, useAction, useApi, usePaymentAccounts } from '@/api/hooks';
import type { DebitCandidate, DebitRunView } from '@/api/types';
import { ListTable } from '@/components/ListTable';
import { confirmAction } from '@/components/ui';
import { date, formatIban, money } from '@/lib/format';
import { SEQUENCES } from '@/lib/sepa';

function DebitModal({ items, opened, onClose }: { items: DebitCandidate[]; opened: boolean; onClose: () => void }) {
  const { data: accounts } = usePaymentAccounts();
  const banks = useMemo(() => (accounts ?? []).filter((a) => a.account.kind === 'bank' && !a.account.archived), [accounts]);
  const [accountId, setAccountId] = useState<string | null>(null);
  useEffect(() => {
    if (!accountId && banks.length) setAccountId(String((banks.find((b) => b.account.iban) ?? banks[0]).account.id));
  }, [banks, accountId]);
  const account = banks.find((b) => String(b.account.id) === accountId)?.account;
  const total = items.reduce((s, c) => s + c.amount, 0);
  const dates = [...new Set(items.map((c) => c.collection_date))].sort();
  const pending = items.filter((c) => c.pending);
  const create = useAction(() => api.post<DebitRunView>('/debit-runs', { account_id: Number(accountId), document_ids: items.map((c) => c.document_id) }), {
    success: (r) => `Lastschriftdatei mit ${r.run.count} Lastschrift(en) erstellt`,
    onSuccess: (r) => {
      onClose();
      downloadFile(`/debit-runs/${r.run.id}/xml`).catch((e) => notifyError(e));
    },
  });
  return (
    <Modal opened={opened} onClose={onClose} size="xl" title={<Text fw={650}>SEPA-Lastschriften einziehen</Text>}>
      <Stack>
        <Select
          label="Gutschrift auf Konto"
          data={banks.map((b) => ({ value: String(b.account.id), label: `${b.account.name}${b.account.iban ? ` · ${formatIban(b.account.iban)}` : ' · keine IBAN'}` }))}
          value={accountId}
          onChange={setAccountId}
          error={account && !account.iban ? 'Für dieses Konto ist keine IBAN hinterlegt' : undefined}
          allowDeselect={false}
        />
        {pending.length > 0 && (
          <Alert color="orange" icon={<IconAlertTriangle size={16} />}>
            {pending.map((c) => c.number).join(', ')} {pending.length === 1 ? 'ist' : 'sind'} bereits in einer Lastschriftdatei enthalten. Bitte nur erneut einziehen, wenn die erste Datei nicht eingereicht wurde.
          </Alert>
        )}
        <Table verticalSpacing={6} horizontalSpacing="xs">
          <Table.Thead>
            <Table.Tr>
              <Table.Th>Rechnung</Table.Th>
              <Table.Th>Kunde / Mandat</Table.Th>
              <Table.Th>Einzug</Table.Th>
              <Table.Th ta="right">Betrag</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {items.map((c) => (
              <Table.Tr key={c.document_id}>
                <Table.Td>{c.number}</Table.Td>
                <Table.Td>
                  <Text size="sm">{c.contact_name}</Text>
                  <Text size="xs" c="dimmed">
                    {c.mandate_reference} · {formatIban(c.iban)}
                  </Text>
                </Table.Td>
                <Table.Td>
                  <Text size="sm">{date(c.collection_date)}</Text>
                  <Text size="xs" c="dimmed">
                    {SEQUENCES[c.sequence] ?? c.sequence}
                    {c.scheme === 'B2B' ? ' · Firmenlastschrift' : ''}
                  </Text>
                </Table.Td>
                <Table.Td ta="right" className="tabular">
                  {money(c.amount)}
                </Table.Td>
              </Table.Tr>
            ))}
            <Table.Tr>
              <Table.Td colSpan={3}>
                <Text fw={650}>Summe ({items.length} Lastschriften)</Text>
              </Table.Td>
              <Table.Td ta="right">
                <Text fw={700} className="tabular">
                  {money(total)}
                </Text>
              </Table.Td>
            </Table.Tr>
          </Table.Tbody>
        </Table>
        <Alert variant="light" color="gray" icon={<IconInfoCircle size={16} />}>
          <Text size="sm">
            Die Datei (pain.008) wird im Online-Banking hochgeladen und muss spätestens einen Bankarbeitstag vor dem Einzug ({dates.map((d) => date(d)).join(', ')}) eingereicht sein. Die Zahlungen werden mit dem Kontoauszug gebucht; Rücklastschriften
            werden dabei erkannt und die Rechnungen wieder geöffnet.
          </Text>
        </Alert>
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button leftSection={<IconFileDownload size={16} />} disabled={!items.length || !accountId} loading={create.isPending} onClick={() => create.mutate()}>
            Datei erstellen
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** Tab on the Offene-Posten page. */
export function DebitTab() {
  const navigate = useNavigate();
  const { data: candidates, isFetching } = useApi<DebitCandidate[]>('/debit-runs/candidates');
  const { data: runs } = useApi<DebitRunView[]>('/debit-runs');
  const [selected, setSelected] = useState<DebitCandidate[]>([]);
  const [open, setOpen] = useState(false);
  const removeRun = useAction((id: number) => api.del(`/debit-runs/${id}`), { success: 'Lastschriftdatei verworfen' });
  const ready = (candidates ?? []).filter((c) => !c.problem && !c.pending);
  const blocked = (candidates ?? []).filter((c) => c.problem);
  return (
    <Stack gap="md">
      {blocked.length > 0 && (
        <Alert color="orange" icon={<IconAlertTriangle size={16} />} title={`${blocked.length} Rechnung(en) können nicht eingezogen werden`}>
          {blocked.map((c) => (
            <Text size="sm" key={c.document_id}>
              {c.number} · {c.contact_name}: {c.problem}
            </Text>
          ))}
        </Alert>
      )}
      <Card padding={0}>
        <Group justify="space-between" p="md">
          <Text size="sm" c="dimmed">
            {selected.length ? `${selected.length} ausgewählt · ${money(selected.reduce((s, c) => s + c.amount, 0))}` : 'Rechnungen per SEPA-Lastschrift auswählen, um eine Lastschriftdatei zu erstellen'}
          </Text>
          <Group gap="xs">
            <Button variant="default" disabled={!ready.length} onClick={() => setSelected(ready.filter((c) => dayjs(c.collection_date).diff(dayjs(), 'day') <= 7))}>
              Fällige auswählen
            </Button>
            <Button leftSection={<IconBuildingBank size={16} />} disabled={!selected.length} onClick={() => setOpen(true)}>
              Lastschriften einziehen{selected.length ? ` (${selected.length})` : ''}
            </Button>
          </Group>
        </Group>
        <ListTable<DebitCandidate>
          records={candidates ?? []}
          fetching={isFetching}
          idAccessor={(c) => c.document_id}
          selectedRecords={selected}
          onSelectedRecordsChange={setSelected}
          isRecordSelectable={(c) => !c.problem}
          defaultSort={{ column: 'collection_date', direction: 'asc' }}
          onRowClick={(c) => navigate(`/dokumente/${c.document_id}`)}
          emptyText="Keine offenen Rechnungen per Lastschrift"
          columns={[
            { accessor: 'number', title: 'Rechnung', render: (c) => <Text size="sm" fw={600}>{c.number}</Text> },
            { accessor: 'contact_name', title: 'Kunde', sortable: true, render: (c) => <><Text size="sm">{c.contact_name}</Text><Text size="xs" c="dimmed">{c.mandate_reference || 'kein Mandat'}</Text></> },
            { accessor: 'collection_date', title: 'Einzug', sortable: true, render: (c) => <><Text size="sm">{date(c.collection_date)}</Text><Text size="xs" c="dimmed">{SEQUENCES[c.sequence] ?? c.sequence}</Text></> },
            {
              accessor: 'status',
              title: 'Status',
              render: (c) =>
                c.problem ? (
                  <Tooltip label={c.problem} multiline w={280}>
                    <Badge size="sm" color="red" variant="light">
                      Problem
                    </Badge>
                  </Tooltip>
                ) : c.pending ? (
                  <Tooltip label="In einer Lastschriftdatei – wartet auf die Gutschrift der Bank">
                    <Badge size="sm" color="blue" variant="light" leftSection={<IconBuildingBank size={11} />}>
                      eingereicht zum {date(c.pending)}
                    </Badge>
                  </Tooltip>
                ) : c.returned ? (
                  <Tooltip label="Die letzte Lastschrift kam zurück. Erneut einziehen oder den Kunden um Überweisung bitten (Mahnwesen).">
                    <Badge size="sm" color="red">
                      Rücklastschrift
                    </Badge>
                  </Tooltip>
                ) : (
                  <Badge size="sm" color="teal" variant="light">
                    bereit
                  </Badge>
                ),
            },
            { accessor: 'amount', title: 'Betrag', sortable: true, textAlign: 'right', render: (c) => <Text size="sm" fw={600} className="tabular">{money(c.amount)}</Text> },
          ]}
        />
      </Card>
      {(runs ?? []).length > 0 && (
        <Card padding={0}>
          <Text fw={650} p="md" pb="xs">
            Lastschriftdateien
          </Text>
          <ListTable<DebitRunView>
            records={runs}
            idAccessor={(r) => r.run.id}
            pageSize={10}
            minHeight={0}
            columns={[
              { accessor: 'run.created_at', title: 'Erstellt', render: (r) => date(r.run.created_at.slice(0, 10)) },
              { accessor: 'collection', title: 'Einzug', render: (r) => [...new Set(r.items.map((i) => i.item.collection_date))].sort().map((d) => date(d)).join(', ') },
              { accessor: 'account_name', title: 'Konto' },
              {
                accessor: 'items',
                title: 'Lastschriften',
                render: (r) => {
                  const paid = r.items.filter((i) => i.paid).length;
                  const returned = r.items.filter((i) => i.returned).length;
                  return (
                    <Tooltip
                      multiline
                      label={r.items.map((i) => (
                        <div key={i.item.id}>
                          {i.document_number} · {i.item.debtor_name} · {money(i.item.amount)}
                          {i.returned ? ' · zurückgegeben' : i.paid ? ' · bezahlt' : ''}
                        </div>
                      ))}
                    >
                      <Text size="sm">
                        {r.run.count} Lastschrift{r.run.count === 1 ? '' : 'en'} · {paid} bezahlt
                        {returned > 0 && (
                          <Text span c="red.7" size="sm">
                            {' '}
                            · {returned} zurück
                          </Text>
                        )}
                      </Text>
                    </Tooltip>
                  );
                },
              },
              { accessor: 'run.total', title: 'Summe', textAlign: 'right', render: (r) => <Text size="sm" fw={600} className="tabular">{money(r.run.total)}</Text> },
              {
                accessor: 'actions',
                title: '',
                width: 90,
                render: (r) => (
                  <Group gap={4} wrap="nowrap" justify="flex-end">
                    <Tooltip label="Datei herunterladen">
                      <ActionIcon variant="subtle" onClick={() => downloadFile(`/debit-runs/${r.run.id}/xml`).catch((e) => notifyError(e))} aria-label="Herunterladen">
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
                            title: 'Lastschriftdatei verwerfen',
                            message: 'Nur verwerfen, wenn die Datei nicht bei der Bank eingereicht wurde. Die Rechnungen bleiben offen und können erneut eingezogen werden.',
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
      <DebitModal
        opened={open}
        items={selected}
        onClose={() => {
          setOpen(false);
          setSelected([]);
        }}
      />
    </Stack>
  );
}
