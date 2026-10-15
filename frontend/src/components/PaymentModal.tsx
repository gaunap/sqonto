import { useEffect, useMemo, useState } from 'react';
import { Alert, Button, Group, Modal, SegmentedControl, Select, Stack, Text } from '@mantine/core';
import { IconInfoCircle } from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction, useApi, usePaymentAccounts } from '@/api/hooks';
import type { PaymentRequest, TransactionView } from '@/api/types';
import { date as fmtDate, money, today } from '@/lib/format';
import { DateField, MoneyInput } from './inputs';

export function PaymentModal({
  opened,
  onClose,
  target,
  id,
  openAmount,
  fees = 0,
  title,
  incoming = true,
}: {
  opened: boolean;
  onClose: () => void;
  target: 'documents' | 'receipts';
  id: number;
  openAmount: number;
  fees?: number;
  title: string;
  incoming?: boolean;
}) {
  const { data: accounts } = usePaymentAccounts();
  const [mode, setMode] = useState<'new' | 'existing'>('new');
  const [accountId, setAccountId] = useState<string | null>(null);
  const [txId, setTxId] = useState<string | null>(null);
  const [day, setDay] = useState<string | null>(today());
  const [amount, setAmount] = useState(openAmount);
  const [fee, setFee] = useState(0);
  const [writeoff, setWriteoff] = useState<string>('');
  const { data: openTx } = useApi<TransactionView[]>(opened ? '/transactions' : null, { status: 'open' });

  useEffect(() => {
    if (opened) {
      setAmount(openAmount);
      setFee(fees);
      setDay(today());
      setWriteoff('');
      setTxId(null);
      setMode('new');
    }
  }, [opened, openAmount, fees]);

  useEffect(() => {
    if (!accountId && accounts?.length) {
      const bank = accounts.find((a) => a.account.kind !== 'cash' && !a.account.archived) ?? accounts[0];
      setAccountId(String(bank.account.id));
    }
  }, [accounts, accountId]);

  const candidates = useMemo(
    () =>
      (openTx ?? [])
        .filter((t) => (incoming ? t.tx.amount > 0 : t.tx.amount < 0))
        .map((t) => ({
          value: String(t.tx.id),
          label: `${fmtDate(t.tx.date)} · ${money(t.tx.amount - t.allocated)} · ${t.tx.counterparty || t.tx.purpose}`.slice(0, 90),
          rest: Math.abs(t.tx.amount - t.allocated),
        }))
        // Best matches first: the movement closest to the open amount.
        .sort((a, b) => Math.abs(a.rest - openAmount) - Math.abs(b.rest - openAmount)),
    [openTx, incoming, openAmount],
  );

  useEffect(() => {
    // An exact amount match is almost certainly the payment – preselect it.
    if (mode === 'existing' && !txId && candidates[0] && Math.abs(candidates[0].rest - openAmount) < 0.005) {
      setTxId(candidates[0].value);
      setAmount(candidates[0].rest);
    }
  }, [mode, txId, candidates, openAmount]);

  const pay = useAction(
    (req: PaymentRequest) => api.post(`/${target}/${id}/payments`, req),
    {
      success: 'Zahlung erfasst',
      onSuccess: onClose,
    },
  );

  const rest = Math.round((openAmount - amount) * 100) / 100;
  const submit = () =>
    pay.mutate({
      account_id: mode === 'new' && accountId ? Number(accountId) : null,
      transaction_id: mode === 'existing' && txId ? Number(txId) : null,
      date: day,
      amount,
      fee_amount: target === 'documents' ? fee : 0,
      writeoff: rest > 0 ? writeoff : '',
      note: '',
    });

  return (
    <Modal opened={opened} onClose={onClose} title={<Text fw={650}>{title}</Text>} size="md">
      <Stack gap="sm">
        <Group justify="space-between">
          <Text size="sm" c="dimmed">
            Offener Betrag
          </Text>
          <Text fw={700} className="tabular">
            {money(openAmount)}
          </Text>
        </Group>
        <SegmentedControl
          value={mode}
          onChange={(v) => setMode(v as 'new' | 'existing')}
          data={[
            { value: 'new', label: 'Zahlung erfassen' },
            { value: 'existing', label: `Kontobewegung zuordnen${candidates.length ? ` (${candidates.length})` : ''}` },
          ]}
        />
        {mode === 'new' ? (
          <>
            <Select
              label="Zahlungskonto"
              data={(accounts ?? []).filter((a) => !a.account.archived).map((a) => ({ value: String(a.account.id), label: `${a.account.name} (${money(a.balance)})` }))}
              value={accountId}
              onChange={setAccountId}
            />
            <DateField label="Zahlungsdatum" value={day} onChange={setDay} />
          </>
        ) : (
          <Select
            label="Offene Kontobewegung"
            placeholder={candidates.length ? 'Bewegung auswählen' : 'Keine passenden offenen Bewegungen'}
            data={candidates}
            value={txId}
            searchable
            onChange={(v) => {
              setTxId(v);
              const c = candidates.find((x) => x.value === v);
              if (c) setAmount(Math.min(c.rest, openAmount));
            }}
          />
        )}
        <MoneyInput label="Betrag" value={amount} onChange={setAmount} min={0} />
        {target === 'documents' && fees > 0 && (
          <MoneyInput label="Zusätzlich erhaltene Mahngebühren / Zinsen" description="Werden als sonstige betriebliche Erträge gebucht" value={fee} onChange={setFee} min={0} />
        )}
        {rest > 0.004 && target === 'documents' && (
          <Stack gap={4}>
            <Text size="sm" fw={500}>
              Differenz von {money(rest)}
            </Text>
            <SegmentedControl
              size="xs"
              value={writeoff}
              onChange={setWriteoff}
              data={[
                { value: '', label: 'Bleibt offen' },
                { value: 'skonto', label: 'Als Skonto ausbuchen' },
                { value: 'loss', label: 'Forderungsverlust' },
              ]}
            />
          </Stack>
        )}
        {writeoff === 'skonto' && rest > 0 && (
          <Alert variant="light" icon={<IconInfoCircle size={16} />} py="xs">
            Der Skontobetrag mindert Umsatz und Umsatzsteuer anteilig.
          </Alert>
        )}
        <Group justify="flex-end" mt="sm">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button onClick={submit} loading={pay.isPending} disabled={amount <= 0 && !writeoff}>
            Zahlung speichern
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
