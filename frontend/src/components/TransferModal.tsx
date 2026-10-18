// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useState } from 'react';
import { Alert, Button, Group, Modal, Select, Stack, Table, Text, TextInput } from '@mantine/core';
import { IconAlertTriangle, IconFileDownload, IconInfoCircle } from '@tabler/icons-react';
import { api, downloadFile } from '@/api/client';
import { notifyError, useAction, usePaymentAccounts } from '@/api/hooks';
import type { PaymentRunView, TransferCandidate } from '@/api/types';
import { DateField, MoneyInput } from '@/components/inputs';
import { date, money, today } from '@/lib/format';

const formatIban = (iban: string) => iban.replace(/(.{4})/g, '$1 ').trim();

/** Creates a SEPA transfer file (pain.001) for the selected open receipts. */
export function TransferModal({ candidates, opened, onClose }: { candidates: TransferCandidate[]; opened: boolean; onClose: () => void }) {
  const { data: accounts } = usePaymentAccounts();
  const banks = useMemo(() => (accounts ?? []).filter((a) => a.account.kind === 'bank' && !a.account.archived), [accounts]);
  const [accountId, setAccountId] = useState<string | null>(null);
  const [execution, setExecution] = useState<string | null>(today());
  const [amounts, setAmounts] = useState<Record<number, number>>({});
  const [texts, setTexts] = useState<Record<number, string>>({});

  useEffect(() => {
    if (opened) {
      setAmounts(Object.fromEntries(candidates.map((c) => [c.receipt_id, c.open_amount])));
      setTexts(Object.fromEntries(candidates.map((c) => [c.receipt_id, c.remittance])));
      setExecution(today());
    }
  }, [opened, candidates]);
  useEffect(() => {
    if (!accountId && banks.length) setAccountId(String((banks.find((b) => b.account.iban) ?? banks[0]).account.id));
  }, [banks, accountId]);

  const ok = candidates.filter((c) => !c.problem);
  const blocked = candidates.filter((c) => c.problem);
  const pending = ok.filter((c) => c.pending);
  const total = ok.reduce((s, c) => s + (amounts[c.receipt_id] ?? 0), 0);
  const account = banks.find((b) => String(b.account.id) === accountId)?.account;

  const create = useAction(
    () =>
      api.post<PaymentRunView>('/payment-runs', {
        account_id: Number(accountId),
        execution_date: execution,
        items: ok.map((c) => ({ receipt_id: c.receipt_id, amount: amounts[c.receipt_id] ?? 0, remittance: texts[c.receipt_id] ?? '' })),
      }),
    {
      success: (r) => `Überweisungsdatei mit ${r.run.count} Überweisung(en) erstellt`,
      onSuccess: (r) => {
        onClose();
        downloadFile(`/payment-runs/${r.run.id}/xml`).catch((e) => notifyError(e));
      },
    },
  );

  return (
    <Modal opened={opened} onClose={onClose} size={1040} title={<Text fw={650}>SEPA-Überweisungen erstellen</Text>}>
      <Stack>
        <Group grow align="flex-start">
          <Select
            label="Auftraggeberkonto"
            data={banks.map((b) => ({ value: String(b.account.id), label: `${b.account.name}${b.account.iban ? ` · ${formatIban(b.account.iban)}` : ' · keine IBAN'}` }))}
            value={accountId}
            onChange={setAccountId}
            error={account && !account.iban ? 'Für dieses Konto ist keine IBAN hinterlegt' : undefined}
            allowDeselect={false}
          />
          <DateField label="Ausführungsdatum" value={execution} onChange={setExecution} minDate={new Date()} />
        </Group>
        {blocked.length > 0 && (
          <Alert color="red" icon={<IconAlertTriangle size={16} />} title={`${blocked.length} Beleg(e) können nicht überwiesen werden`}>
            {blocked.map((c) => (
              <Text size="sm" key={c.receipt_id}>
                {c.number} · {c.name}: {c.problem}
              </Text>
            ))}
          </Alert>
        )}
        {pending.length > 0 && (
          <Alert color="orange" icon={<IconAlertTriangle size={16} />}>
            {pending.map((c) => `${c.number} (${c.name})`).join(', ')} {pending.length === 1 ? 'ist' : 'sind'} bereits in einer Überweisungsdatei zur Ausführung am {date(pending[0].pending)} enthalten. Bitte nur erneut überweisen, wenn die
            erste Datei nicht an die Bank übergeben wurde.
          </Alert>
        )}
        {ok.length > 0 && (
          <Table verticalSpacing={6} horizontalSpacing="xs">
            <Table.Thead>
              <Table.Tr>
                <Table.Th w="34%">Empfänger</Table.Th>
                <Table.Th>Verwendungszweck</Table.Th>
                <Table.Th ta="right" w={140}>
                  Betrag
                </Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {ok.map((c) => (
                <Table.Tr key={c.receipt_id}>
                  <Table.Td>
                    <Text size="sm" fw={500}>
                      {c.name}
                    </Text>
                    <Text size="xs" c="dimmed" ff="monospace">
                      {formatIban(c.iban)}
                    </Text>
                    <Text size="xs" c="dimmed">
                      {c.number} · offen {money(c.open_amount)}
                      {c.due_date && ` · fällig ${date(c.due_date)}`}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <TextInput size="xs" maxLength={140} value={texts[c.receipt_id] ?? ''} onChange={(e) => setTexts((t) => ({ ...t, [c.receipt_id]: e.currentTarget.value }))} />
                  </Table.Td>
                  <Table.Td>
                    <MoneyInput
                      size="xs"
                      value={amounts[c.receipt_id] ?? 0}
                      onChange={(v) => setAmounts((a) => ({ ...a, [c.receipt_id]: v }))}
                      error={(amounts[c.receipt_id] ?? 0) > c.open_amount + 0.005 ? 'höher als offen' : undefined}
                    />
                  </Table.Td>
                </Table.Tr>
              ))}
              <Table.Tr>
                <Table.Td colSpan={2}>
                  <Text fw={650}>Summe ({ok.length} Überweisungen)</Text>
                </Table.Td>
                <Table.Td ta="right">
                  <Text fw={700} className="tabular">
                    {money(total)}
                  </Text>
                </Table.Td>
              </Table.Tr>
            </Table.Tbody>
          </Table>
        )}
        <Alert variant="light" color="gray" icon={<IconInfoCircle size={16} />}>
          <Text size="sm">
            Die Datei (pain.001) wird im Online-Banking hochgeladen und dort freigegeben. Für Skonto den Betrag anpassen. Die Zahlung wird erst mit dem Kontoauszug gebucht – beim Bankimport werden die Überweisungen den Belegen zugeordnet.
          </Text>
        </Alert>
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button leftSection={<IconFileDownload size={16} />} disabled={ok.length === 0 || !accountId || !execution} loading={create.isPending} onClick={() => create.mutate()}>
            Datei erstellen
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
