// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router';
import { ActionIcon, Badge, Button, Card, Group, Modal, Select, SimpleGrid, Stack, Switch, Text, TextInput, Textarea, Tooltip } from '@mantine/core';
import { IconEdit, IconPlus, IconSearch } from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction, useChart, useStatus } from '@/api/hooks';
import type { Account } from '@/api/types';
import { ListTable } from '@/components/ListTable';
import { Page, PageHeader } from '@/components/ui';
import { ACCOUNT_KINDS } from '@/lib/labels';

const EUER_OPTIONS = [
  ['', '– nicht EÜR-relevant –'],
  ['be_ust', 'BE: Umsatzsteuerpflichtige Betriebseinnahmen'],
  ['be_frei', 'BE: Umsatzsteuerfreie Betriebseinnahmen'],
  ['be_anlagen', 'BE: Veräußerung von Anlagevermögen'],
  ['ba_waren', 'BA: Waren, Roh- und Hilfsstoffe'],
  ['ba_fremd', 'BA: Fremdleistungen'],
  ['ba_personal', 'BA: Personal'],
  ['ba_raum', 'BA: Raumkosten'],
  ['ba_telekom', 'BA: Telekommunikation'],
  ['ba_reise', 'BA: Reisekosten'],
  ['ba_fortbildung', 'BA: Fortbildung'],
  ['ba_beratung', 'BA: Rechts- und Steuerberatung'],
  ['ba_miete_bew', 'BA: Miete/Leasing bewegliche WG'],
  ['ba_versicherung', 'BA: Beiträge, Versicherungen'],
  ['ba_werbung', 'BA: Werbekosten'],
  ['ba_kfz', 'BA: Kfz-Kosten'],
  ['ba_bewirtung', 'BA: Bewirtung'],
  ['ba_sonst', 'BA: Übrige Betriebsausgaben'],
  ['nabz', 'Nicht abziehbar'],
].map(([value, label]) => ({ value, label }));

const GUV_OPTIONS = [
  ['', '–'],
  ['g01', '1. Umsatzerlöse'],
  ['g04', '4. Sonstige betriebliche Erträge'],
  ['g05a', '5a. Material/Waren'],
  ['g05b', '5b. Bezogene Leistungen'],
  ['g06a', '6a. Löhne und Gehälter'],
  ['g07', '7. Abschreibungen'],
  ['g08', '8. Sonstige betriebliche Aufwendungen'],
  ['g11', '11. Zinserträge'],
  ['g13', '13. Zinsaufwendungen'],
  ['g16', '16. Sonstige Steuern'],
].map(([value, label]) => ({ value, label }));

const BWA_OPTIONS = [
  ['umsatz', 'Umsatzerlöse'],
  ['sonst_erloese', 'Sonstige betriebliche Erlöse'],
  ['material', 'Material- / Wareneinkauf'],
  ['fremd', 'Fremdleistungen'],
  ['personal', 'Personalkosten'],
  ['raum', 'Raumkosten'],
  ['steuern_betr', 'Betriebliche Steuern'],
  ['versich', 'Versicherungen / Beiträge'],
  ['kfz', 'Kfz-Kosten'],
  ['werbe_reise', 'Werbe- / Reisekosten'],
  ['afa', 'Abschreibungen'],
  ['reparatur', 'Reparatur / Instandhaltung'],
  ['sonstige', 'Sonstige Kosten'],
  ['zinsaufw', 'Zinsaufwand'],
  ['neutral_aufw', 'Sonstiger neutraler Aufwand'],
  ['zinsertr', 'Zinserträge'],
  ['neutral_ertr', 'Sonstiger neutraler Ertrag'],
  ['steuern_ee', 'Steuern Einkommen und Ertrag'],
].map(([value, label]) => ({ value, label }));

const BILANZ_OPTIONS = [
  { group: 'Aktiva', items: [
    ['a_immat', 'A. I. Immaterielle Vermögensgegenstände'],
    ['a_sach', 'A. II. Sachanlagen'],
    ['a_finanz', 'A. III. Finanzanlagen'],
    ['a_vorraete', 'B. I. Vorräte'],
    ['a_ford', 'B. II. 1. Forderungen aus L+L'],
    ['a_sonst_vg', 'B. II. 4. Sonstige Vermögensgegenstände'],
    ['a_liquide', 'B. IV. Kasse, Bank'],
    ['a_rap', 'C. Rechnungsabgrenzungsposten'],
  ] },
  { group: 'Passiva', items: [
    ['p_ek', 'A. Eigenkapital'],
    ['p_rueck', 'B. Rückstellungen'],
    ['p_vb_kredit', 'C. 2. Verbindlichkeiten ggü. Kreditinstituten'],
    ['p_anz', 'C. 3. Erhaltene Anzahlungen'],
    ['p_vb_ll', 'C. 4. Verbindlichkeiten aus L+L'],
    ['p_vb_sonst', 'C. 8. Sonstige Verbindlichkeiten'],
    ['p_rap', 'D. Rechnungsabgrenzungsposten'],
  ] },
].map((g) => ({ group: g.group, items: g.items.map(([value, label]) => ({ value, label })) }));

const TAX_RATE_OPTIONS = [
  { value: '', label: 'keine Vorgabe' },
  { value: '19', label: '19 %' },
  { value: '7', label: '7 %' },
  { value: '0', label: '0 % / steuerfrei' },
];

/** Sensible mappings when the account type of a new account changes. */
const KIND_DEFAULTS: Record<string, Partial<Account>> = {
  expense: { euer: 'ba_sonst', guv: 'g08', bwa: 'sonstige', bilanz: '', tax_rate: 19, pick: 'expense' },
  revenue: { euer: 'be_ust', guv: 'g01', bwa: 'umsatz', bilanz: '', tax_rate: 19, pick: 'revenue' },
  asset: { euer: '', guv: '', bwa: '', bilanz: 'a_sach', tax_rate: 19, pick: 'asset' },
  current: { euer: '', guv: '', bwa: '', bilanz: 'a_sonst_vg', tax_rate: null, pick: 'other' },
  liability: { euer: '', guv: '', bwa: '', bilanz: 'p_vb_sonst', tax_rate: null, pick: 'other' },
  equity: { euer: '', guv: '', bwa: '', bilanz: 'p_ek', tax_rate: null, pick: 'other' },
  private: { euer: '', guv: '', bwa: '', bilanz: 'p_ek', tax_rate: null, pick: 'private' },
};

function AccountModal({ opened, onClose, account }: { opened: boolean; onClose: () => void; account: Account | null }) {
  const [a, setA] = useState<Account | null>(null);
  const current: Account =
    a ??
    account ?? {
      id: 0,
      number: '',
      name: '',
      kind: 'expense',
      euer: 'ba_sonst',
      guv: 'g08',
      bwa: 'sonstige',
      bilanz: '',
      tax_rate: 19,
      tax_code: '',
      role: '',
      datev_auto: false,
      pick: 'expense',
      hint: '',
      system: false,
      active: true,
    };
  const set = (p: Partial<Account>) => setA({ ...current, ...p });
  const save = useAction((x: Account) => (x.id ? api.put(`/accounts/${x.id}`, x) : api.post('/accounts', x)), {
    success: 'Konto gespeichert',
    onSuccess: () => {
      setA(null);
      onClose();
    },
  });
  const close = () => {
    setA(null);
    onClose();
  };
  return (
    <Modal opened={opened} onClose={close} size="lg" title={<Text fw={650}>{current.id ? `Konto ${current.number}` : 'Neues Konto'}</Text>}>
      <Stack gap="sm">
        <SimpleGrid cols={2} spacing="sm">
          <TextInput label="Kontonummer" value={current.number} onChange={(e) => set({ number: e.currentTarget.value })} disabled={!!current.id} />
          <Select
            label="Kontoart"
            disabled={current.system}
            data={Object.entries(ACCOUNT_KINDS).map(([value, label]) => ({ value, label }))}
            value={current.kind}
            onChange={(v) => {
              const kind = v ?? 'expense';
              set({ kind, ...(current.id ? { pick: ['expense', 'revenue', 'asset', 'private'].includes(kind) ? kind : 'other' } : (KIND_DEFAULTS[kind] ?? { euer: '', guv: '', bwa: '', bilanz: '', pick: 'other' })) });
            }}
          />
        </SimpleGrid>
        <TextInput label="Bezeichnung" value={current.name} onChange={(e) => set({ name: e.currentTarget.value })} />
        {(current.kind === 'expense' || current.kind === 'revenue') && (
          <SimpleGrid cols={2} spacing="sm">
            <Select label="EÜR-Zuordnung" data={EUER_OPTIONS} value={current.euer} onChange={(v) => set({ euer: v ?? '' })} disabled={current.system} />
            <Select label="GuV-Position" data={GUV_OPTIONS} value={current.guv} onChange={(v) => set({ guv: v ?? '' })} disabled={current.system} />
            <Select label="BWA-Zeile" data={BWA_OPTIONS} value={current.bwa || null} onChange={(v) => set({ bwa: v ?? '' })} disabled={current.system} placeholder="automatisch" clearable />
            <Select
              label="Steuersatz-Vorschlag"

              data={TAX_RATE_OPTIONS}
              value={current.tax_rate == null ? '' : String(current.tax_rate)}
              onChange={(v) => set({ tax_rate: v ? Number(v) : null })}
              disabled={current.system}
            />
          </SimpleGrid>
        )}
        {['asset', 'current', 'liability', 'equity', 'private'].includes(current.kind) && (
          <SimpleGrid cols={2} spacing="sm">
            <Select label="Bilanzposition" data={BILANZ_OPTIONS} value={current.bilanz || null} onChange={(v) => set({ bilanz: v ?? '' })} disabled={current.system} placeholder="–" clearable />
            {current.kind === 'asset' && (
              <Select label="Steuersatz-Vorschlag" data={TAX_RATE_OPTIONS} value={current.tax_rate == null ? '' : String(current.tax_rate)} onChange={(v) => set({ tax_rate: v ? Number(v) : null })} disabled={current.system} />
            )}
          </SimpleGrid>
        )}
        <Textarea label="Hinweis / Beschreibung" value={current.hint} onChange={(e) => set({ hint: e.currentTarget.value })} />
        {!current.system && <Switch label="Aktiv" checked={current.active} onChange={(e) => set({ active: e.currentTarget.checked })} />}
        {current.system && (
          <Text size="xs" c="dimmed">
            Systemkonto – Nummer und Zuordnungen werden für automatische Buchungen benötigt.
          </Text>
        )}
        <Group justify="flex-end">
          <Button variant="default" onClick={close}>
            Abbrechen
          </Button>
          <Button onClick={() => save.mutate(current)} loading={save.isPending}>
            Speichern
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

export function AccountsPage() {
  const navigate = useNavigate();
  const { data: status } = useStatus();
  const { data } = useChart();
  const [kind, setKind] = useState<string | null>(null);
  const [q, setQ] = useState('');
  const [modal, setModal] = useState<{ open: boolean; account: Account | null }>({ open: false, account: null });
  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    return (data ?? []).filter((a) => (!kind || a.kind === kind) && (!s || `${a.number} ${a.name} ${a.hint}`.toLowerCase().includes(s)));
  }, [data, kind, q]);
  return (
    <Page>
      <PageHeader
        title="Kontenplan"
        subtitle={`Kontenrahmen ${status?.chart ?? ''} – Konto anklicken, um das Kontenblatt zu öffnen`}
        actions={
          <Button leftSection={<IconPlus size={16} />} onClick={() => setModal({ open: true, account: null })}>
            Konto anlegen
          </Button>
        }
      />
      <Stack gap="md">
        <Group>
          <Select placeholder="Alle Kontoarten" data={Object.entries(ACCOUNT_KINDS).map(([value, label]) => ({ value, label }))} value={kind} onChange={setKind} clearable w={220} />
          <TextInput placeholder="Nummer oder Bezeichnung …" leftSection={<IconSearch size={16} />} value={q} onChange={(e) => setQ(e.currentTarget.value)} w={280} />
        </Group>
        <Card padding={0}>
          <ListTable<Account>
            records={filtered}
            defaultSort={{ column: 'number', direction: 'asc' }}
            pageSize={50}
            onRowClick={(a) => navigate(`/buchhaltung/konten/${a.number}`)}
            columns={[
              { accessor: 'number', title: 'Konto', sortable: true, width: 90, render: (a) => <Text size="sm" fw={700} className="tabular">{a.number}</Text> },
              {
                accessor: 'name',
                title: 'Bezeichnung',
                sortable: true,
                render: (a) => (
                  <div>
                    <Group gap={6} wrap="nowrap">
                      <Text size="sm" c={a.active ? undefined : 'dimmed'}>
                        {a.name}
                      </Text>
                      {a.system && <Badge size="xs" variant="outline">System</Badge>}
                      {a.datev_auto && <Badge size="xs" variant="outline" color="violet">Automatik</Badge>}
                      {!a.active && <Badge size="xs" color="gray">inaktiv</Badge>}
                    </Group>
                    {a.hint && (
                      <Text size="xs" c="dimmed">
                        {a.hint}
                      </Text>
                    )}
                  </div>
                ),
              },
              { accessor: 'kind', title: 'Art', sortable: true, width: 170, render: (a) => <Badge variant="light" color="gray">{ACCOUNT_KINDS[a.kind] ?? a.kind}</Badge> },
              { accessor: 'tax_rate', title: 'USt', sortable: true, width: 80, textAlign: 'right', render: (a) => <Text size="sm" className="tabular">{a.tax_rate != null ? `${a.tax_rate} %` : '–'}</Text> },
              {
                accessor: 'edit',
                title: '',
                width: 60,
                textAlign: 'right',
                render: (a) => (
                  <Tooltip label="Bearbeiten">
                    <ActionIcon onClick={(e) => { e.stopPropagation(); setModal({ open: true, account: a }); }} aria-label="Konto bearbeiten">
                      <IconEdit size={16} />
                    </ActionIcon>
                  </Tooltip>
                ),
              },
            ]}
          />
        </Card>
      </Stack>
      <AccountModal opened={modal.open} onClose={() => setModal({ open: false, account: null })} account={modal.account} />
    </Page>
  );
}
