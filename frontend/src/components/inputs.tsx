// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useMemo, useState, type FocusEvent, type KeyboardEvent, type MouseEvent } from 'react';
import { Button, Group, NumberInput, Select, Text, type NumberInputProps, type SelectProps, Badge } from '@mantine/core';
import { DateInput, type DateInputProps } from '@mantine/dates';
import { IconCalendar, IconPlus } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { useChart, useContacts } from '@/api/hooks';
import type { Account, Contact } from '@/api/types';
import { ACCOUNT_KINDS } from '@/lib/labels';
import { ContactDrawer } from './ContactDrawer';

/** Date input working with ISO strings (YYYY-MM-DD). */
export function DateField({ value, onChange, ...rest }: Omit<DateInputProps, 'value' | 'onChange'> & { value: string | null | undefined; onChange: (v: string | null) => void }) {
  return (
    <DateInput
      value={value ?? null}
      onChange={(v) => onChange(v ? dayjs(v).format('YYYY-MM-DD') : null)}
      valueFormat="DD.MM.YYYY"
      dateParser={(input) => {
        const d = dayjs(input, ['DD.MM.YYYY', 'D.M.YYYY', 'DD.MM.YY', 'D.M.YY', 'YYYY-MM-DD'], true);
        return d.isValid() ? d.format('YYYY-MM-DD') : null;
      }}
      leftSection={<IconCalendar size={16} />}
      popoverProps={{ withinPortal: true }}
      {...rest}
    />
  );
}

/**
 * Select the whole value when an input gets focus, so typing replaces it. The mouseup that
 * follows a click would otherwise collapse the selection and leave the caret in front of
 * the decimals (typing "1200" into "0,00" then yields 12.000,00).
 */
export const selectAllOnFocus = {
  // Capture phase: NumberInput (react-number-format) defers its own onFocus via setTimeout.
  onFocusCapture: (e: FocusEvent<HTMLInputElement>) => {
    e.currentTarget.select();
    e.currentTarget.dataset.selectAll = '1';
  },
  onMouseUp: (e: MouseEvent<HTMLInputElement>) => {
    if (e.currentTarget.dataset.selectAll) {
      e.preventDefault();
      delete e.currentTarget.dataset.selectAll;
    }
  },
  onKeyDown: (e: KeyboardEvent<HTMLInputElement>) => {
    delete e.currentTarget.dataset.selectAll;
  },
};

export function MoneyInput(props: Omit<NumberInputProps, 'onChange' | 'value'> & { value: number | null | undefined; onChange: (v: number) => void; currency?: boolean }) {
  const { value, onChange, currency = true, ...rest } = props;
  return (
    <NumberInput
      value={value ?? 0}
      onChange={(v) => onChange(typeof v === 'number' ? v : parseFloat(String(v).replace(',', '.')) || 0)}
      decimalScale={2}
      fixedDecimalScale
      decimalSeparator=","
      thousandSeparator="."
      hideControls
      {...selectAllOnFocus}
      rightSection={currency ? <Text size="sm" c="dimmed" pr={4}>€</Text> : undefined}
      rightSectionWidth={currency ? 28 : undefined}
      styles={{ input: { textAlign: 'right', fontVariantNumeric: 'tabular-nums' } }}
      {...rest}
    />
  );
}

export function DecimalInput(props: Omit<NumberInputProps, 'onChange' | 'value'> & { value: number | null | undefined; onChange: (v: number) => void; scale?: number }) {
  const { value, onChange, scale = 3, ...rest } = props;
  return (
    <NumberInput
      value={value ?? 0}
      onChange={(v) => onChange(typeof v === 'number' ? v : parseFloat(String(v).replace(',', '.')) || 0)}
      decimalScale={scale}
      decimalSeparator=","
      thousandSeparator="."
      hideControls
      {...selectAllOnFocus}
      styles={{ input: { textAlign: 'right', fontVariantNumeric: 'tabular-nums' } }}
      {...rest}
    />
  );
}

export function displayName(c: Pick<Contact, 'kind' | 'company' | 'first_name' | 'last_name'>) {
  if (c.kind === 'person' || !c.company.trim()) {
    const n = `${c.first_name} ${c.last_name}`.trim();
    return n || c.company;
  }
  return c.company;
}

/** Searchable contact picker with "create new" shortcut. */
export function ContactSelect({
  value,
  onChange,
  role,
  label = 'Kontakt',
  ...rest
}: Omit<SelectProps, 'data' | 'value' | 'onChange'> & { value: number | null; onChange: (id: number | null, contact?: Contact) => void; role?: 'customer' | 'supplier' }) {
  const { data } = useContacts();
  const [creating, setCreating] = useState(false);
  const options = useMemo(
    () =>
      (data ?? [])
        .filter((c) => !role || (role === 'customer' ? c.contact.is_customer : c.contact.is_supplier) || c.contact.id === value)
        .map((c) => ({ value: String(c.contact.id), label: c.name, contact: c.contact })),
    [data, role, value],
  );
  return (
    <>
      <Select
        label={label}
        placeholder="Kontakt suchen …"
        searchable
        clearable
        nothingFoundMessage="Kein Kontakt gefunden"
        selectFirstOptionOnChange
        data={options}
        value={value ? String(value) : null}
        onChange={(v) => {
          const c = options.find((o) => o.value === v)?.contact;
          onChange(v ? Number(v) : null, c);
        }}
        renderOption={({ option }) => {
          const c = (option as (typeof options)[number]).contact;
          return (
            <Group gap="xs" wrap="nowrap" justify="space-between" w="100%">
              <div style={{ minWidth: 0 }}>
                <Text size="sm" truncate>
                  {option.label}
                </Text>
                <Text size="xs" c="dimmed" truncate>
                  {[c.number, c.zip && `${c.zip} ${c.city}`].filter(Boolean).join(' · ')}
                </Text>
              </div>
              {c.is_supplier && !c.is_customer && <Badge size="xs" color="grape">Lieferant</Badge>}
            </Group>
          );
        }}
        {...rest}
      />
      <Button variant="subtle" size="compact-xs" leftSection={<IconPlus size={12} />} mt={4} onClick={() => setCreating(true)}>
        Neuen Kontakt anlegen
      </Button>
      <ContactDrawer
        opened={creating}
        onClose={() => setCreating(false)}
        defaultRole={role}
        onSaved={(c) => {
          setCreating(false);
          onChange(c.id, c);
        }}
      />
    </>
  );
}

const PICK_ORDER = ['expense', 'asset', 'revenue', 'tax', 'private', 'transfer', 'bank', 'cash', 'other'];
const PICK_LABEL: Record<string, string> = {
  expense: 'Ausgaben',
  asset: 'Anlagevermögen',
  revenue: 'Einnahmen / Erlöse',
  tax: 'Steuern',
  private: 'Privat',
  transfer: 'Umbuchung',
  bank: 'Bank',
  cash: 'Kasse',
  other: 'Sonstige Konten',
};

/** Account (category) picker, grouped. */
export function AccountSelect({
  value,
  onChange,
  picks,
  allAccounts = false,
  ...rest
}: Omit<SelectProps, 'data' | 'value' | 'onChange'> & { value: string | null; onChange: (v: string | null, account?: Account) => void; picks?: string[]; allAccounts?: boolean }) {
  const { data } = useChart();
  const groups = useMemo(() => {
    const accounts = (data ?? []).filter((a) => a.active);
    if (allAccounts) {
      const byKind = new Map<string, Account[]>();
      for (const a of accounts) {
        const list = byKind.get(a.kind) ?? [];
        list.push(a);
        byKind.set(a.kind, list);
      }
      return [...byKind.entries()].map(([k, list]) => ({
        group: ACCOUNT_KINDS[k] ?? k,
        items: list.map((a) => ({ value: a.number, label: `${a.number} · ${a.name}` })),
      }));
    }
    const allowed = picks ?? ['expense', 'asset'];
    return PICK_ORDER.filter((p) => allowed.includes(p))
      .map((p) => ({
        group: PICK_LABEL[p],
        items: accounts.filter((a) => a.pick === p).map((a) => ({ value: a.number, label: `${a.name} (${a.number})` })),
      }))
      .filter((g) => g.items.length > 0);
  }, [data, picks, allAccounts]);
  return (
    <Select
      searchable
      data={groups}
      value={value || null}
      onChange={(v) => onChange(v, data?.find((a) => a.number === v))}
      nothingFoundMessage="Kein Konto gefunden"
      selectFirstOptionOnChange
      maxDropdownHeight={360}
      comboboxProps={{ width: 420, position: 'bottom-start' }}
      {...rest}
    />
  );
}

export function YearSelect({ value, onChange, from = 2020 }: { value: number; onChange: (v: number) => void; from?: number }) {
  const current = new Date().getFullYear();
  const years = [];
  for (let y = current + 1; y >= from; y--) years.push({ value: String(y), label: String(y) });
  return <Select data={years} value={String(value)} onChange={(v) => v && onChange(Number(v))} w={110} aria-label="Jahr" />;
}
