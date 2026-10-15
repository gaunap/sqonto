import { useState } from 'react';
import { useNavigate, useParams } from 'react-router';
import { Anchor, Avatar, Badge, Button, Card, Center, Grid, Group, Loader, Menu, SimpleGrid, Stack, Tabs, Text } from '@mantine/core';
import { IconArchive, IconChevronDown, IconEdit, IconFileInvoice, IconFileText, IconMail, IconPhone, IconReceipt2, IconWorld, IconClipboardCheck } from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction, useApi } from '@/api/hooks';
import type { ContactOverview } from '@/api/types';
import { ContactDrawer } from '@/components/ContactDrawer';
import { displayName } from '@/components/inputs';
import { ListTable } from '@/components/ListTable';
import { InfoRow, KpiCard, Page, PageHeader, SectionCard, StatusBadge, confirmAction } from '@/components/ui';
import { date, formatIban, money } from '@/lib/format';
import { countryName, docLabel, EINVOICE_FORMATS, TAX_MODES } from '@/lib/labels';
import { initials } from './ContactsPage';

export function ContactDetailPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { data, refetch } = useApi<ContactOverview>(`/contacts/${id}`);
  const [editing, setEditing] = useState(false);
  const remove = useAction(() => api.del<{ message: string }>(`/contacts/${id}`), {
    success: (r) => (r.message === 'archived' ? 'Kontakt archiviert (es existieren Dokumente)' : 'Kontakt gelöscht'),
    onSuccess: () => navigate('/kontakte'),
  });

  if (!data) {
    return (
      <Center h="60vh">
        <Loader />
      </Center>
    );
  }
  const c = data.contact;
  const name = displayName(c);
  const newDoc = (type: string) => navigate(`/dokumente/neu?type=${type}&contact=${c.id}`);

  return (
    <Page>
      <PageHeader
        back={{ to: '/kontakte', label: 'Kontakte' }}
        title={
          <Group gap="sm">
            <Avatar color={c.kind === 'person' ? 'teal' : 'brand'} radius="xl" size={44}>
              {initials(name)}
            </Avatar>
            {name}
          </Group>
        }
        badge={
          <Group gap={4}>
            {c.is_customer && <Badge>Kunde</Badge>}
            {c.is_supplier && <Badge color="grape">Lieferant</Badge>}
            {c.archived && <Badge color="gray">Archiviert</Badge>}
          </Group>
        }
        subtitle={`Nr. ${c.number}${c.city ? ` · ${c.zip} ${c.city}` : ''}`}
        actions={
          <>
            <Button variant="default" leftSection={<IconEdit size={16} />} onClick={() => setEditing(true)}>
              Bearbeiten
            </Button>
            <Menu position="bottom-end">
              <Menu.Target>
                <Button rightSection={<IconChevronDown size={14} />}>Neues Dokument</Button>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Item leftSection={<IconFileInvoice size={16} />} onClick={() => newDoc('invoice')}>
                  Rechnung
                </Menu.Item>
                <Menu.Item leftSection={<IconFileText size={16} />} onClick={() => newDoc('quote')}>
                  Angebot
                </Menu.Item>
                <Menu.Item leftSection={<IconClipboardCheck size={16} />} onClick={() => newDoc('order')}>
                  Auftragsbestätigung
                </Menu.Item>
                {c.is_supplier && (
                  <Menu.Item leftSection={<IconReceipt2 size={16} />} onClick={() => navigate('/belege/neu')}>
                    Beleg erfassen
                  </Menu.Item>
                )}
                <Menu.Divider />
                <Menu.Item
                  color="red"
                  leftSection={<IconArchive size={16} />}
                  onClick={() => confirmAction({ title: 'Kontakt entfernen', message: 'Kontakte mit Dokumenten werden archiviert, alle anderen gelöscht.', danger: true, confirmLabel: 'Entfernen', onConfirm: () => remove.mutate() })}
                >
                  Löschen / Archivieren
                </Menu.Item>
              </Menu.Dropdown>
            </Menu>
          </>
        }
      />
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }} mb="lg">
        <KpiCard label="Umsatz laufendes Jahr" value={money(data.revenue_year)} />
        <KpiCard label="Umsatz gesamt" value={money(data.revenue_total)} />
        <KpiCard label="Offene Forderungen" value={money(data.open_amount)} color="orange" />
        <KpiCard label="Ausgaben (Belege)" value={money(data.expenses_total)} />
      </SimpleGrid>
      <Grid gap="lg">
        <Grid.Col span={{ base: 12, lg: 4 }}>
          <Stack gap="lg">
            <SectionCard title="Kontaktdaten">
              <Text size="sm" style={{ whiteSpace: 'pre-line' }} mb="sm">
                {[c.company && c.kind === 'organization' ? c.company : `${c.salutation} ${c.title} ${c.first_name} ${c.last_name}`.trim(), c.address_extra, c.street, `${c.zip} ${c.city}`.trim(), c.country !== 'DE' ? countryName(c.country) : '']
                  .filter(Boolean)
                  .join('\n')}
              </Text>
              <Stack gap={6}>
                {c.email && (
                  <Anchor href={`mailto:${c.email}`} size="sm">
                    <Group gap={6}>
                      <IconMail size={14} />
                      {c.email}
                    </Group>
                  </Anchor>
                )}
                {c.phone && (
                  <Anchor href={`tel:${c.phone}`} size="sm">
                    <Group gap={6}>
                      <IconPhone size={14} />
                      {c.phone}
                    </Group>
                  </Anchor>
                )}
                {c.website && (
                  <Anchor href={c.website.startsWith('http') ? c.website : `https://${c.website}`} target="_blank" size="sm">
                    <Group gap={6}>
                      <IconWorld size={14} />
                      {c.website}
                    </Group>
                  </Anchor>
                )}
              </Stack>
            </SectionCard>
            {c.persons.length > 0 && (
              <SectionCard title="Ansprechpartner">
                <Stack gap="sm">
                  {c.persons.map((p, i) => (
                    <Group key={i} gap="sm" wrap="nowrap">
                      <Avatar size={32} radius="xl" color="gray">
                        {initials(`${p.first_name} ${p.last_name}`)}
                      </Avatar>
                      <div>
                        <Text size="sm" fw={500}>
                          {[p.salutation, p.title, p.first_name, p.last_name].filter(Boolean).join(' ')}
                          {p.primary && (
                            <Badge size="xs" ml={6} variant="light">
                              Haupt
                            </Badge>
                          )}
                        </Text>
                        <Text size="xs" c="dimmed">
                          {[p.position, p.email, p.phone].filter(Boolean).join(' · ')}
                        </Text>
                      </div>
                    </Group>
                  ))}
                </Stack>
              </SectionCard>
            )}
            <SectionCard title="Steuern & Zahlung">
              <InfoRow label="USt-IdNr.">{c.vat_id || '–'}</InfoRow>
              <InfoRow label="Steuernummer">{c.tax_number || '–'}</InfoRow>
              <InfoRow label="Steuerart">{TAX_MODES.find((t) => t.value === c.default_tax_mode)?.label ?? 'automatisch'}</InfoRow>
              <InfoRow label="Zahlungsziel">{c.payment_terms_days != null ? `${c.payment_terms_days} Tage` : 'Standard'}</InfoRow>
              <InfoRow label="IBAN">{c.iban ? formatIban(c.iban) : '–'}</InfoRow>
              <InfoRow label="Rechnungsformat">{EINVOICE_FORMATS.find((f) => f.value === c.einvoice_format)?.label}</InfoRow>
              {c.leitweg_id && <InfoRow label="Leitweg-ID">{c.leitweg_id}</InfoRow>}
              {c.tags.length > 0 && (
                <Group gap={4} mt="sm">
                  {c.tags.map((t) => (
                    <Badge key={t} variant="outline" color="gray">
                      {t}
                    </Badge>
                  ))}
                </Group>
              )}
              {c.notes && (
                <Text size="sm" c="dimmed" mt="sm" style={{ whiteSpace: 'pre-line' }}>
                  {c.notes}
                </Text>
              )}
            </SectionCard>
          </Stack>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 8 }}>
          <Card padding={0}>
            <Tabs defaultValue={data.documents.length || !data.receipts.length ? 'documents' : 'receipts'}>
              <Tabs.List px="md" pt="xs">
                <Tabs.Tab value="documents">Dokumente ({data.documents.length})</Tabs.Tab>
                <Tabs.Tab value="receipts">Belege ({data.receipts.length})</Tabs.Tab>
                <Tabs.Tab value="history">Verlauf</Tabs.Tab>
              </Tabs.List>
              <Tabs.Panel value="documents">
                <ListTable
                  records={data.documents}
                  defaultSort={{ column: 'date', direction: 'desc' }}
                  onRowClick={(d) => navigate(`/dokumente/${d.id}`)}
                  emptyText="Noch keine Dokumente"
                  columns={[
                    { accessor: 'number', title: 'Nummer', render: (d) => <Text size="sm" fw={600}>{d.number || 'Entwurf'}</Text> },
                    { accessor: 'doc_type', title: 'Art', render: (d) => docLabel(d.doc_type, d.kind) },
                    { accessor: 'date', title: 'Datum', sortable: true, render: (d) => date(d.date) },
                    { accessor: 'gross_total', title: 'Betrag', textAlign: 'right', sortable: true, render: (d) => <Text size="sm" className="tabular">{money(d.gross_total)}</Text> },
                    { accessor: 'status', title: 'Status', render: (d) => <StatusBadge status={d.status} docType={d.doc_type} daysOverdue={d.days_overdue} size="sm" /> },
                  ]}
                />
              </Tabs.Panel>
              <Tabs.Panel value="receipts">
                <ListTable
                  records={data.receipts}
                  idAccessor={(r) => r.receipt.id}
                  defaultSort={{ column: 'receipt.date', direction: 'desc' }}
                  onRowClick={(r) => navigate(`/belege/${r.receipt.id}`)}
                  emptyText="Noch keine Belege"
                  columns={[
                    { accessor: 'receipt.number', title: 'Beleg', render: (r) => <Text size="sm" fw={600}>{r.receipt.number}</Text> },
                    { accessor: 'receipt.invoice_number', title: 'Re.-Nr.' },
                    { accessor: 'receipt.date', title: 'Datum', sortable: true, render: (r) => date(r.receipt.date) },
                    { accessor: 'receipt.gross_total', title: 'Betrag', textAlign: 'right', render: (r) => <Text size="sm" className="tabular">{money(r.receipt.gross_total)}</Text> },
                    { accessor: 'receipt.status', title: 'Status', render: (r) => <StatusBadge status={r.receipt.status} docType="invoice" size="sm" /> },
                  ]}
                />
              </Tabs.Panel>
              <Tabs.Panel value="history" p="md">
                <Stack gap="xs">
                  {data.activities.length === 0 && <Text size="sm" c="dimmed">Keine Einträge</Text>}
                  {data.activities.map((a) => (
                    <Text key={a.id} size="sm">
                      <Text span c="dimmed" size="xs" mr={8}>
                        {date(a.created_at)}
                      </Text>
                      {a.message}
                    </Text>
                  ))}
                </Stack>
              </Tabs.Panel>
            </Tabs>
          </Card>
        </Grid.Col>
      </Grid>
      <ContactDrawer opened={editing} onClose={() => setEditing(false)} contact={c} onSaved={() => { setEditing(false); refetch(); }} />
    </Page>
  );
}
