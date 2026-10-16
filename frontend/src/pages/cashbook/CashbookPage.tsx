// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'react-router';
import { ActionIcon, Badge, Button, Card, Group, Modal, SegmentedControl, Select, SimpleGrid, Stack, Text, TextInput, Tooltip } from '@mantine/core';
import { MonthPickerInput } from '@mantine/dates';
import { IconArrowBackUp, IconCash, IconChevronLeft, IconChevronRight, IconDownload, IconMinus, IconPlus } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api, downloadFile } from '@/api/client';
import { notifyError, useAction, useApi, usePaymentAccounts, useSettings } from '@/api/hooks';
import type { Cashbook, CashbookRow, DocumentListItem, PaymentAccount, ReceiptListItem } from '@/api/types';
import { AccountSelect, DateField, MoneyInput } from '@/components/inputs';
import { ListTable } from '@/components/ListTable';
import { Empty, KpiCard, Page, PageHeader, confirmAction } from '@/components/ui';
import { date, money, today } from '@/lib/format';
import { TAX_RATES } from '@/lib/labels';
import { PaymentAccountModal } from '../banking/BankPage';

function CashEntryModal({ opened, onClose, accountId, direction }: { opened: boolean; onClose: () => void; accountId: number; direction: 'in' | 'out' }) {
  const { data: settings } = useSettings();
  const [dir, setDir] = useState<'in' | 'out'>(direction);
  const [day, setDay] = useState<string | null>(today());
  const [amount, setAmount] = useState(0);
  const [text, setText] = useState('');
  const [account, setAccount] = useState<string | null>(null);
  const [rate, setRate] = useState('19');
  const [link, setLink] = useState<string | null>(null);
  const { data: docs } = useApi<DocumentListItem[]>(opened ? '/documents' : null, { doc_type: 'invoice', status: 'unpaid' });
  const { data: receipts } = useApi<ReceiptListItem[]>(opened ? '/receipts' : null, { status: 'unpaid' });
  useEffect(() => {
    if (opened) {
      setDir(direction);
      setDay(today());
      setAmount(0);
      setText('');
      setAccount(null);
      setLink(null);
    }
  }, [opened, direction]);
  const linkOptions = useMemo(
    () =>
      dir === 'in'
        ? (docs ?? []).filter((d) => d.status !== 'draft').map((d) => ({ value: `d${d.id}`, label: `Rechnung ${d.number} · ${d.contact_name} · ${money(d.open_amount)}`, amount: d.open_amount, text: `Barzahlung Rechnung ${d.number}` }))
        : (receipts ?? []).filter((r) => r.receipt.receipt_type === 'expense').map((r) => ({ value: `r${r.receipt.id}`, label: `Beleg ${r.receipt.number} · ${r.party} · ${money(r.open_amount)}`, amount: r.open_amount, text: r.receipt.description || r.party })),
    [dir, docs, receipts],
  );
  const save = useAction(
    () =>
      api.post('/cashbook', {
        account_id: accountId,
        date: day,
        direction: dir,
        amount,
        description: text,
        counter_account: account ?? '',
        tax_rate: Number(rate),
        receipt_id: link?.startsWith('r') ? Number(link.slice(1)) : null,
        document_id: link?.startsWith('d') ? Number(link.slice(1)) : null,
      }),
    { success: 'Kassenbuchung erfasst', onSuccess: onClose },
  );
  return (
    <Modal opened={opened} onClose={onClose} title={<Text fw={650}>Kassenbuchung</Text>}>
      <Stack gap="sm">
        <SegmentedControl
          value={dir}
          onChange={(v) => {
            setDir(v as 'in' | 'out');
            setLink(null);
          }}
          color={dir === 'in' ? 'teal' : 'red'}
          data={[
            { value: 'in', label: 'Einnahme' },
            { value: 'out', label: 'Ausgabe' },
          ]}
        />
        <SimpleGrid cols={2} spacing="sm">
          <DateField label="Datum" value={day} onChange={setDay} />
          <MoneyInput label="Betrag (brutto)" value={amount} onChange={setAmount} min={0} data-autofocus />
        </SimpleGrid>
        <Select
          label={dir === 'in' ? 'Offene Rechnung bar bezahlt (optional)' : 'Vorhandenen Beleg bar bezahlt (optional)'}
          data={linkOptions}
          value={link}
          clearable
          searchable
          onChange={(v) => {
            setLink(v);
            const o = linkOptions.find((x) => x.value === v);
            if (o) {
              setAmount(o.amount);
              setText(o.text);
            }
          }}
        />
        <TextInput label="Buchungstext" required value={text} onChange={(e) => setText(e.currentTarget.value)} placeholder={dir === 'in' ? 'z. B. Barverkauf' : 'z. B. Briefmarken'} />
        {!link && (
          <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
            <div style={{ gridColumn: 'span 2' }}>
              <AccountSelect
                label="Kategorie"
                picks={dir === 'in' ? ['revenue', 'transfer', 'private', 'other'] : ['expense', 'transfer', 'private', 'tax', 'other']}
                value={account}
                onChange={(v, a) => {
                  setAccount(v);
                  if (a?.tax_rate != null) setRate(String(a.tax_rate));
                }}
              />
            </div>
            <Select label="USt" data={TAX_RATES} value={rate} onChange={(v) => setRate(v ?? '0')} disabled={settings?.tax.small_business} />
          </SimpleGrid>
        )}
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button color={dir === 'in' ? 'teal' : 'red'} onClick={() => save.mutate()} loading={save.isPending} disabled={amount <= 0 || !text || (!link && !account)}>
            Buchen
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

export function CashbookPage() {
  const [params, setParams] = useSearchParams();
  const { data: accounts } = usePaymentAccounts();
  const cashAccounts = (accounts ?? []).filter((a) => a.account.kind === 'cash');
  const [accountId, setAccountId] = useState<number | null>(null);
  const [month, setMonth] = useState<string | null>(dayjs().startOf('month').format('YYYY-MM-DD'));
  const [entry, setEntry] = useState<{ open: boolean; dir: 'in' | 'out' }>({ open: false, dir: 'in' });
  const [newCash, setNewCash] = useState(false);

  useEffect(() => {
    if (!accountId && cashAccounts.length) setAccountId(cashAccounts[0].account.id);
  }, [cashAccounts, accountId]);
  useEffect(() => {
    if (params.get('neu') === '1' && accountId) {
      setEntry({ open: true, dir: 'out' });
      setParams({}, { replace: true });
    }
  }, [params, accountId, setParams]);

  const from = month ? dayjs(month).startOf('month').format('YYYY-MM-DD') : undefined;
  const to = month ? dayjs(month).endOf('month').format('YYYY-MM-DD') : undefined;
  const { data, isFetching } = useApi<Cashbook>(accountId ? '/cashbook' : null, { account_id: accountId ?? undefined, from, to }, { keepPrevious: true });
  const reverse = useAction((id: number) => api.post(`/cashbook/${id}/reverse`), { success: 'Stornobuchung erstellt' });

  if (accounts && cashAccounts.length === 0) {
    const empty: PaymentAccount = { id: 0, name: 'Kasse', kind: 'cash', iban: '', bic: '', bank_name: '', ledger_account: '', opening_balance: 0, opening_date: `${new Date().getFullYear()}-01-01`, color: 'teal', archived: false, created_at: '' };
    return (
      <Page>
        <PageHeader title="Kassenbuch" />
        <Card>
          <Empty
            icon={<IconCash size={22} />}
            title="Noch keine Kasse angelegt"
            description="Führen Sie Bargeldeinnahmen und -ausgaben GoBD-konform mit fortlaufender Nummerierung."
            action={<Button onClick={() => setNewCash(true)}>Kasse anlegen</Button>}
          />
        </Card>
        <PaymentAccountModal opened={newCash} onClose={() => setNewCash(false)} account={empty} />
      </Page>
    );
  }

  return (
    <Page>
      <PageHeader
        title="Kassenbuch"
        subtitle="Bargeschäfte fortlaufend erfassen – Korrekturen erfolgen per Stornobuchung (GoBD)"
        actions={
          <>
            <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile('/cashbook/pdf', { account_id: accountId ?? undefined, from, to }).catch((e) => notifyError(e))}>
              Kassenbericht (PDF)
            </Button>
            <Button color="red" variant="light" leftSection={<IconMinus size={16} />} onClick={() => setEntry({ open: true, dir: 'out' })}>
              Ausgabe
            </Button>
            <Button color="teal" leftSection={<IconPlus size={16} />} onClick={() => setEntry({ open: true, dir: 'in' })}>
              Einnahme
            </Button>
          </>
        }
      />
      <Stack gap="md">
        <Group>
          {cashAccounts.length > 1 && <Select data={cashAccounts.map((a) => ({ value: String(a.account.id), label: a.account.name }))} value={accountId ? String(accountId) : null} onChange={(v) => setAccountId(v ? Number(v) : null)} w={220} />}
          <Group gap={4}>
            <ActionIcon variant="default" size="lg" onClick={() => setMonth(dayjs(month).subtract(1, 'month').format('YYYY-MM-DD'))} aria-label="Vormonat">
              <IconChevronLeft size={16} />
            </ActionIcon>
            <MonthPickerInput value={month} onChange={setMonth} valueFormat="MMMM YYYY" w={180} aria-label="Monat" />
            <ActionIcon variant="default" size="lg" onClick={() => setMonth(dayjs(month).add(1, 'month').format('YYYY-MM-DD'))} aria-label="Folgemonat">
              <IconChevronRight size={16} />
            </ActionIcon>
          </Group>
        </Group>
        <SimpleGrid cols={{ base: 2, md: 4 }}>
          <KpiCard label="Anfangsbestand" value={money(data?.opening ?? 0)} loading={!data} />
          <KpiCard label="Einnahmen" value={money(data?.income ?? 0)} color="teal" loading={!data} />
          <KpiCard label="Ausgaben" value={money(data?.expense ?? 0)} color="red" loading={!data} />
          <KpiCard label="Endbestand" value={money(data?.closing ?? 0)} color="cyan" loading={!data} />
        </SimpleGrid>
        <Card padding={0}>
          <ListTable<CashbookRow>
            records={data?.rows}
            idAccessor={(r) => r.tx.tx.id}
            fetching={isFetching}
            noPagination
            emptyText="Keine Kassenbuchungen in diesem Monat"
            rowColor={(r) => (r.reversed || r.tx.tx.reversal_of ? 'var(--mantine-color-gray-0)' : undefined)}
            columns={[
              { accessor: 'tx.tx.cash_number', title: 'Nr.', width: 60, render: (r) => <Text size="sm" c="dimmed">{r.tx.tx.cash_number}</Text> },
              { accessor: 'tx.tx.date', title: 'Datum', width: 105, render: (r) => date(r.tx.tx.date) },
              {
                accessor: 'tx.tx.purpose',
                title: 'Buchungstext',
                render: (r) => (
                  <Group gap={6}>
                    <Text size="sm" td={r.reversed ? 'line-through' : undefined}>
                      {r.tx.tx.purpose}
                    </Text>
                    {r.reversed && <Badge size="xs" color="gray">storniert</Badge>}
                    {r.tx.tx.reversal_of && <Badge size="xs" color="orange">Storno</Badge>}
                  </Group>
                ),
              },
              { accessor: 'category', title: 'Kategorie', visibleMediaQuery: '(min-width: 62em)', render: (r) => <Text size="xs" c="dimmed" lineClamp={1}>{r.tx.allocations.map((a) => a.label).join(', ')}</Text> },
              { accessor: 'in', title: 'Einnahme', textAlign: 'right', render: (r) => (r.tx.tx.amount > 0 ? <Text size="sm" c="teal.7" className="tabular">{money(r.tx.tx.amount)}</Text> : '') },
              { accessor: 'out', title: 'Ausgabe', textAlign: 'right', render: (r) => (r.tx.tx.amount < 0 ? <Text size="sm" c="red.7" className="tabular">{money(-r.tx.tx.amount)}</Text> : '') },
              { accessor: 'balance', title: 'Bestand', textAlign: 'right', render: (r) => <Text size="sm" fw={600} className="tabular">{money(r.balance)}</Text> },
              {
                accessor: 'actions',
                title: '',
                width: 50,
                render: (r) =>
                  !r.reversed && !r.tx.tx.reversal_of ? (
                    <Tooltip label="Stornieren">
                      <ActionIcon
                        onClick={() =>
                          confirmAction({
                            title: 'Kassenbuchung stornieren',
                            message: `Für Buchung Nr. ${r.tx.tx.cash_number} wird eine Gegenbuchung erstellt.`,
                            confirmLabel: 'Stornieren',
                            danger: true,
                            onConfirm: () => reverse.mutate(r.tx.tx.id),
                          })
                        }
                        aria-label="Stornieren"
                      >
                        <IconArrowBackUp size={16} />
                      </ActionIcon>
                    </Tooltip>
                  ) : null,
              },
            ]}
          />
        </Card>
      </Stack>
      {accountId && <CashEntryModal opened={entry.open} onClose={() => setEntry({ ...entry, open: false })} accountId={accountId} direction={entry.dir} />}
    </Page>
  );
}
