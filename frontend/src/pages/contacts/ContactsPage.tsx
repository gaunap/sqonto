import { useEffect, useMemo, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router';
import { Avatar, Badge, Button, Card, Group, SegmentedControl, Stack, Switch, Text, TextInput } from '@mantine/core';
import { IconAddressBook, IconFileExport, IconPlus, IconSearch } from '@tabler/icons-react';
import { useApi } from '@/api/hooks';
import type { ContactListItem } from '@/api/types';
import { ContactDrawer } from '@/components/ContactDrawer';
import { ListTable } from '@/components/ListTable';
import { Empty, Page, PageHeader } from '@/components/ui';
import { exportCsv } from '@/lib/csv';
import { money } from '@/lib/format';
import { countryName } from '@/lib/labels';

export function initials(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase())
    .join('');
}

export function ContactsPage() {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [role, setRole] = useState('all');
  const [archived, setArchived] = useState(false);
  const [q, setQ] = useState('');
  const [creating, setCreating] = useState(params.get('neu') === '1');
  const { data, isFetching } = useApi<ContactListItem[]>('/contacts', { role: role === 'all' ? undefined : role, archived }, { keepPrevious: true });

  useEffect(() => {
    if (params.get('neu') === '1') {
      setCreating(true);
      setParams({}, { replace: true });
    }
  }, [params, setParams]);

  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    return (data ?? []).filter((c) => !s || `${c.name} ${c.contact.number} ${c.contact.email} ${c.contact.city} ${c.contact.tags.join(' ')}`.toLowerCase().includes(s));
  }, [data, q]);

  return (
    <Page>
      <PageHeader
        title="Kontakte"
        subtitle="Kunden und Lieferanten mit Anschrift, Ansprechpartnern und Zahlungsbedingungen"
        actions={
          <>
            <Button
              variant="default"
              leftSection={<IconFileExport size={16} />}
              onClick={() =>
                exportCsv('Kontakte', filtered, [
                  { label: 'Nummer', value: (c) => c.contact.number },
                  { label: 'Name', value: (c) => c.name },
                  { label: 'Straße', value: (c) => c.contact.street },
                  { label: 'PLZ', value: (c) => c.contact.zip },
                  { label: 'Ort', value: (c) => c.contact.city },
                  { label: 'Land', value: (c) => c.contact.country },
                  { label: 'E-Mail', value: (c) => c.contact.email },
                  { label: 'Telefon', value: (c) => c.contact.phone },
                  { label: 'USt-IdNr.', value: (c) => c.contact.vat_id },
                  { label: 'Umsatz netto', value: (c) => c.revenue },
                ])
              }
            >
              Export
            </Button>
            <Button leftSection={<IconPlus size={16} />} onClick={() => setCreating(true)}>
              Neuer Kontakt
            </Button>
          </>
        }
      />
      <Stack gap="md">
        <Group justify="space-between">
          <Group gap="md">
            <SegmentedControl
              value={role}
              onChange={setRole}
              data={[
                { value: 'all', label: 'Alle' },
                { value: 'customer', label: 'Kunden' },
                { value: 'supplier', label: 'Lieferanten' },
              ]}
            />
            <Switch label="Archivierte anzeigen" checked={archived} onChange={(e) => setArchived(e.currentTarget.checked)} />
          </Group>
          <TextInput placeholder="Name, Nummer, Ort, E-Mail …" leftSection={<IconSearch size={16} />} value={q} onChange={(e) => setQ(e.currentTarget.value)} w={280} />
        </Group>
        <Card padding={0}>
          {data && data.length === 0 && !q && role === 'all' && !archived ? (
            <Empty
              icon={<IconAddressBook size={22} />}
              title="Noch keine Kontakte"
              description="Legen Sie Kunden und Lieferanten an, um Rechnungen und Belege zuzuordnen."
              action={
                <Button leftSection={<IconPlus size={16} />} onClick={() => setCreating(true)}>
                  Kontakt anlegen
                </Button>
              }
            />
          ) : (
            <ListTable<ContactListItem>
              records={filtered}
              idAccessor={(c) => c.contact.id}
              fetching={isFetching}
              defaultSort={{ column: 'name', direction: 'asc' }}
              onRowClick={(c) => navigate(`/kontakte/${c.contact.id}`)}
              columns={[
                {
                  accessor: 'name',
                  title: 'Name',
                  sortable: true,
                  render: (c) => (
                    <Group gap="sm" wrap="nowrap">
                      <Avatar color={c.contact.kind === 'person' ? 'teal' : 'brand'} radius="xl" size={34}>
                        {initials(c.name)}
                      </Avatar>
                      <Stack gap={0} style={{ minWidth: 0 }}>
                        <Text size="sm" fw={600} truncate>
                          {c.name}
                        </Text>
                        <Text size="xs" c="dimmed" truncate>
                          {c.contact.persons.find((p) => p.primary) ? `${c.contact.persons.find((p) => p.primary)?.first_name} ${c.contact.persons.find((p) => p.primary)?.last_name}` : c.contact.email}
                        </Text>
                      </Stack>
                    </Group>
                  ),
                },
                { accessor: 'contact.number', title: 'Nr.', sortable: true, width: 90 },
                {
                  accessor: 'contact.city',
                  title: 'Ort',
                  sortable: true,
                  visibleMediaQuery: '(min-width: 48em)',
                  render: (c) => (
                    <Text size="sm">
                      {[c.contact.zip, c.contact.city].filter(Boolean).join(' ')}
                      {c.contact.country !== 'DE' && (
                        <Text span size="xs" c="dimmed" ml={6}>
                          {countryName(c.contact.country)}
                        </Text>
                      )}
                    </Text>
                  ),
                },
                {
                  accessor: 'role',
                  title: 'Typ',
                  width: 170,
                  render: (c) => (
                    <Group gap={4}>
                      {c.contact.is_customer && <Badge size="sm">Kunde</Badge>}
                      {c.contact.is_supplier && (
                        <Badge size="sm" color="grape">
                          Lieferant
                        </Badge>
                      )}
                      {c.contact.einvoice_format !== 'pdf' && (
                        <Badge size="sm" color="violet" variant="outline">
                          {c.contact.einvoice_format === 'xrechnung' ? 'XRechnung' : 'ZUGFeRD'}
                        </Badge>
                      )}
                    </Group>
                  ),
                },
                { accessor: 'revenue', title: 'Umsatz', sortable: true, textAlign: 'right', render: (c) => <Text size="sm" className="tabular">{c.revenue ? money(c.revenue) : '–'}</Text> },
                {
                  accessor: 'open_amount',
                  title: 'Offen',
                  sortable: true,
                  textAlign: 'right',
                  render: (c) => (
                    <Text size="sm" className="tabular" c={c.open_amount > 0 ? 'orange.8' : 'dimmed'}>
                      {c.open_amount ? money(c.open_amount) : '–'}
                    </Text>
                  ),
                },
              ]}
            />
          )}
        </Card>
      </Stack>
      <ContactDrawer opened={creating} onClose={() => setCreating(false)} onSaved={(c) => { setCreating(false); navigate(`/kontakte/${c.id}`); }} />
    </Page>
  );
}
