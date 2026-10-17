// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useNavigate, useParams } from 'react-router';
import {
  ActionIcon,
  Alert,
  Badge,
  Box,
  Button,
  Card,
  FileButton,
  Grid,
  Group,
  Image,
  Menu,
  Modal,
  NumberInput,
  Paper,
  Radio,
  Select,
  SimpleGrid,
  Stack,
  Switch,
  Table,
  Tabs,
  Text,
  TextInput,
  Textarea,
  ThemeIcon,
} from '@mantine/core';
import {
  IconAlertTriangle,
  IconArchive,
  IconBell,
  IconBuilding,
  IconBuildingBank,
  IconCopy,
  IconDatabase,
  IconInfoCircle,
  IconDeviceFloppy,
  IconDots,
  IconEdit,
  IconFileText,
  IconHash,
  IconHome2,
  IconLayout,
  IconLock,
  IconPlus,
  IconReceiptTax,
  IconRefresh,
  IconStar,
  IconTrash,
  IconUpload,
  IconCash,
} from '@tabler/icons-react';
import { api, apiUrl, downloadFile } from '@/api/client';
import { notifyError, useAction, useApi, useLayouts, usePaymentAccounts, useSettings, useStatus } from '@/api/hooks';
import type { FileMeta, Layout, MessageResponse, NumberRange, PaymentAccount, Settings } from '@/api/types';
import { DateField, MoneyInput } from '@/components/inputs';
import { Page, PageHeader, SqontoLogo, UnsavedChangesGuard, confirmAction } from '@/components/ui';
import { date, money } from '@/lib/format';
import { BOOKKEEPING, COUNTRIES, EINVOICE_FORMATS } from '@/lib/labels';
import { PaymentAccountModal } from '../banking/BankPage';

type Patch = (fn: (s: Settings) => Settings) => void;

function CompanyTab({ s, patch }: { s: Settings; patch: Patch }) {
  const c = s.company;
  const set = (p: Partial<Settings['company']>) => patch((x) => ({ ...x, company: { ...x.company, ...p } }));
  const [uploading, setUploading] = useState(false);
  const uploadLogo = async (f: File | null) => {
    if (!f) return;
    setUploading(true);
    try {
      const meta = await api.upload<FileMeta>('/files', f);
      set({ logo_file_id: meta.id });
    } catch (e) {
      notifyError(e);
    } finally {
      setUploading(false);
    }
  };
  return (
    <Grid gap="lg">
      <Grid.Col span={{ base: 12, lg: 8 }}>
        <Card>
          <Stack gap="sm">
            <Text fw={650}>Firma & Anschrift</Text>
            <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
              <TextInput label="Firmenname" required value={c.name} onChange={(e) => set({ name: e.currentTarget.value })} />
              <Select label="Rechtsform" data={['Einzelunternehmen', 'Freiberufler', 'GbR', 'GmbH', 'UG (haftungsbeschränkt)', 'GmbH & Co. KG', 'KG', 'OHG', 'e.K.', 'AG', 'e.V.']} value={c.legal_form || null} onChange={(v) => set({ legal_form: v ?? '' })} searchable />
              <TextInput label="Inhaber / Geschäftsführung" value={c.owner} onChange={(e) => set({ owner: e.currentTarget.value })} />
              <TextInput label="Ansprechpartner (Rechnungen)" value={c.contact_name} onChange={(e) => set({ contact_name: e.currentTarget.value })} />
              <TextInput label="Straße und Hausnummer" value={c.street} onChange={(e) => set({ street: e.currentTarget.value })} />
              <SimpleGrid cols={2} spacing="sm">
                <TextInput label="PLZ" value={c.zip} onChange={(e) => set({ zip: e.currentTarget.value })} />
                <TextInput label="Ort" value={c.city} onChange={(e) => set({ city: e.currentTarget.value })} />
              </SimpleGrid>
              <Select label="Land" data={COUNTRIES} value={c.country} onChange={(v) => set({ country: v ?? 'DE' })} searchable />
              <TextInput label="Telefon" value={c.phone} onChange={(e) => set({ phone: e.currentTarget.value })} />
              <TextInput label="E-Mail" value={c.email} onChange={(e) => set({ email: e.currentTarget.value })} />
              <TextInput label="Website" value={c.website} onChange={(e) => set({ website: e.currentTarget.value })} />
            </SimpleGrid>
            <Text fw={650} mt="md">
              Steuern & Register
            </Text>
            <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
              <TextInput label="Steuernummer" value={c.tax_number} onChange={(e) => set({ tax_number: e.currentTarget.value })} />
              <TextInput label="USt-IdNr." value={c.vat_id} onChange={(e) => set({ vat_id: e.currentTarget.value })} />
              <TextInput label="Registergericht" value={c.register_court} onChange={(e) => set({ register_court: e.currentTarget.value })} />
              <TextInput label="Registernummer" value={c.register_number} onChange={(e) => set({ register_number: e.currentTarget.value })} />
            </SimpleGrid>
            <Text fw={650} mt="md">
              Bankverbindung (erscheint auf Rechnungen)
            </Text>
            <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
              <TextInput label="Bank" value={c.bank_name} onChange={(e) => set({ bank_name: e.currentTarget.value })} />
              <TextInput label="IBAN" value={c.iban} onChange={(e) => set({ iban: e.currentTarget.value })} />
              <TextInput label="BIC" value={c.bic} onChange={(e) => set({ bic: e.currentTarget.value })} />
            </SimpleGrid>
          </Stack>
        </Card>
      </Grid.Col>
      <Grid.Col span={{ base: 12, lg: 4 }}>
        <Card>
          <Text fw={650} mb="sm">
            Logo
          </Text>
          <Paper withBorder h={140} radius="md" p="md" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center' }} bg="white">
            {c.logo_file_id ? <Image src={apiUrl(`/files/${c.logo_file_id}`)} mah={110} fit="contain" alt="Logo" /> : <Text size="sm" c="dimmed">Kein Logo hinterlegt</Text>}
          </Paper>
          <Group mt="sm">
            <FileButton onChange={uploadLogo} accept="image/png,image/jpeg,image/svg+xml,image/webp">
              {(props) => (
                <Button {...props} variant="light" leftSection={<IconUpload size={16} />} loading={uploading}>
                  Logo hochladen
                </Button>
              )}
            </FileButton>
            {c.logo_file_id && (
              <Button variant="subtle" color="red" onClick={() => set({ logo_file_id: null })}>
                Entfernen
              </Button>
            )}
          </Group>
          <Text size="xs" c="dimmed" mt="sm">
            PNG mit transparentem Hintergrund oder SVG empfohlen. Position und Größe legen Sie im Layout-Designer fest.
          </Text>
        </Card>
      </Grid.Col>
    </Grid>
  );
}

/** Legal forms that are merchants by law (Kaufleute) and must keep double-entry books. */
const MERCHANT_FORMS = ['GmbH', 'UG (haftungsbeschränkt)', 'GmbH & Co. KG', 'KG', 'OHG', 'e.K.', 'AG'];

function TaxTab({ s, patch }: { s: Settings; patch: Patch }) {
  const { data: status } = useStatus();
  const t = s.tax;
  const set = (p: Partial<Settings['tax']>) => patch((x) => ({ ...x, tax: { ...x.tax, ...p } }));
  const rebuild = useAction(() => api.post<MessageResponse>('/system/rebuild'), { success: (r) => r.message });
  return (
    <Stack gap="lg" maw={900}>
      <Card>
        <Text fw={650} mb="sm">
          Buchführung
        </Text>
        <Radio.Group value={t.bookkeeping} onChange={(v) => set({ bookkeeping: v })}>
          <SimpleGrid cols={{ base: 1, sm: 2 }}>
            {BOOKKEEPING.map((b) => (
              <Radio.Card key={b.value} value={b.value} p="md" radius="md" disabled={!!t.locked_until}>
                <Group wrap="nowrap" align="flex-start">
                  <Radio.Indicator />
                  <div>
                    <Text fw={600} size="sm">
                      {b.label}
                    </Text>
                    <Text size="xs" c="dimmed" mt={4}>
                      {b.description}
                    </Text>
                  </div>
                </Group>
              </Radio.Card>
            ))}
          </SimpleGrid>
        </Radio.Group>
        {t.bookkeeping === 'euer' && MERCHANT_FORMS.includes(s.company.legal_form) && (
          <Alert color="yellow" icon={<IconAlertTriangle size={16} />} mt="md" p="sm">
            <Text size="sm">
              Als {s.company.legal_form} sind Sie nach § 238 HGB zur doppelten Buchführung verpflichtet. Die Einnahmen-Überschuss-Rechnung ist nur für Freiberufler, Kleingewerbetreibende und nicht eingetragene Einzelunternehmen zulässig.
            </Text>
          </Alert>
        )}
        <SimpleGrid cols={{ base: 1, sm: 2 }} mt="md" spacing="sm">
          <Select
            label="Kontenrahmen"
            data={[
              { value: 'SKR03', label: 'SKR03 (Prozessgliederung)' },
              { value: 'SKR04', label: 'SKR04 (Abschlussgliederung)' },
            ]}
            value={t.chart}
            onChange={(v) => set({ chart: v ?? 'SKR03' })}
            description={status?.has_data ? 'Kann nach dem Erfassen von Buchungen nicht mehr geändert werden' : undefined}
            inputWrapperOrder={['label', 'input', 'description', 'error']}
            disabled={status?.has_data}
          />
          <NumberInput label="Standard-Umsatzsteuersatz" suffix=" %" value={t.default_tax_rate} onChange={(v) => set({ default_tax_rate: Number(v) || 19 })} />
        </SimpleGrid>
      </Card>
      <Card>
        <Text fw={650} mb="sm">
          Umsatzsteuer
        </Text>
        <Stack gap="sm">
          <Switch label="Kleinunternehmer nach § 19 UStG" description="Es wird keine Umsatzsteuer ausgewiesen und keine Vorsteuer abgezogen" checked={t.small_business} onChange={(e) => set({ small_business: e.currentTarget.checked })} disabled={!!t.locked_until} />
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
            <Select
              label="Voranmeldungszeitraum"
              data={[
                { value: 'monthly', label: 'Monatlich' },
                { value: 'quarterly', label: 'Vierteljährlich' },
                { value: 'yearly', label: 'Jährlich (keine Voranmeldung)' },
              ]}
              value={t.ustva_period}
              onChange={(v) => set({ ustva_period: v ?? 'quarterly' })}
            />
            <Switch mt="lg" label="Dauerfristverlängerung" description="Fälligkeit verschiebt sich um einen Monat" checked={t.permanent_extension} onChange={(e) => set({ permanent_extension: e.currentTarget.checked })} />
          </SimpleGrid>
          <Text size="xs" c="dimmed">
            Versteuerung: {t.bookkeeping === 'bilanz' ? 'Soll-Versteuerung (nach vereinbarten Entgelten)' : 'Ist-Versteuerung (nach vereinnahmten Entgelten)'}
          </Text>
        </Stack>
      </Card>
      <Card>
        <Group gap="sm" mb="sm">
          <ThemeIcon variant="light" color="orange">
            <IconLock size={16} />
          </ThemeIcon>
          <Text fw={650}>Festschreibung (GoBD)</Text>
        </Group>
        <Text size="sm" c="dimmed" mb="sm">
          Festgeschriebene Zeiträume können nicht mehr verändert werden – z. B. nach Abgabe der Umsatzsteuer-Voranmeldung oder des Jahresabschlusses.
        </Text>
        <Group align="flex-end">
          <DateField label="Festgeschrieben bis einschließlich" value={t.locked_until} onChange={(v) => set({ locked_until: v })} clearable w={260} />
        </Group>
      </Card>
      <Card>
        <Text fw={650} mb={4}>
          Buchungen neu erzeugen
        </Text>
        <Text size="sm" c="dimmed" mb="sm">
          Erzeugt das Buchungsjournal aus allen Rechnungen, Belegen, Zahlungen und Anlagen neu. Normalerweise nicht nötig.
        </Text>
        <Button variant="light" leftSection={<IconRefresh size={16} />} onClick={() => rebuild.mutate()} loading={rebuild.isPending}>
          Journal neu erzeugen
        </Button>
      </Card>
    </Stack>
  );
}

const TEXT_KEYS: { key: string; label: string }[] = [
  { key: 'invoice', label: 'Rechnung' },
  { key: 'quote', label: 'Angebot' },
  { key: 'order', label: 'Auftragsbestätigung' },
  { key: 'delivery', label: 'Lieferschein' },
  { key: 'partial', label: 'Abschlagsrechnung' },
  { key: 'final', label: 'Schlussrechnung' },
  { key: 'credit_note', label: 'Gutschrift' },
  { key: 'cancellation', label: 'Stornorechnung' },
];

function DocsTab({ s, patch }: { s: Settings; patch: Patch }) {
  const { data: layouts } = useLayouts();
  const d = s.docs;
  const set = (p: Partial<Settings['docs']>) => patch((x) => ({ ...x, docs: { ...x.docs, ...p } }));
  const [key, setKey] = useState('invoice');
  const texts = d.texts[key] ?? { title: '', intro: '', closing: '' };
  const setText = (p: Partial<typeof texts>) => set({ texts: { ...d.texts, [key]: { ...texts, ...p } } });
  return (
    <Stack gap="lg" maw={1000}>
      <Card>
        <Text fw={650} mb="sm">
          Standardwerte
        </Text>
        <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
          <NumberInput label="Zahlungsziel" suffix=" Tage" min={0} value={d.payment_terms_days} onChange={(v) => set({ payment_terms_days: Number(v) || 0 })} />
          <NumberInput label="Gültigkeit von Angeboten" suffix=" Tage" min={1} value={d.quote_validity_days} onChange={(v) => set({ quote_validity_days: Number(v) || 30 })} />
          <TextInput label="Standard-Einheit" value={d.default_unit} onChange={(e) => set({ default_unit: e.currentTarget.value })} />
          <Select label="Standard-Rechnungsformat" data={EINVOICE_FORMATS} value={d.einvoice_default} onChange={(v) => set({ einvoice_default: v ?? 'pdf' })} />
          <Select
            label="Standard-Layout"
            data={(layouts ?? []).map((l) => ({ value: String(l.id), label: l.name }))}
            value={String(d.default_layout_id ?? layouts?.find((l) => l.is_default)?.id ?? layouts?.[0]?.id ?? '') || null}
            onChange={(v) => set({ default_layout_id: v ? Number(v) : null })}
            allowDeselect={false}
          />
        </SimpleGrid>
      </Card>
      <Card>
        <Text fw={650} mb={4}>
          Textvorlagen
        </Text>
        <Text size="sm" c="dimmed" mb="sm">
          Platzhalter: {'{{Anrede}}, {{Kunde}}, {{Nummer}}, {{Datum}}, {{Betrag}}, {{Fällig am}}, {{Zahlungsziel}}, {{Gültig bis}}, {{Skonto}}, {{Firma}}, {{Referenz}}, {{Bezugsnummer}}'}
        </Text>
        <Tabs value={key} onChange={(v) => setKey(v ?? 'invoice')} variant="pills">
          <Tabs.List mb="md">
            {TEXT_KEYS.map((t) => (
              <Tabs.Tab key={t.key} value={t.key}>
                {t.label}
              </Tabs.Tab>
            ))}
          </Tabs.List>
        </Tabs>
        <Stack gap="sm">
          <TextInput label="Titel" value={texts.title} onChange={(e) => setText({ title: e.currentTarget.value })} />
          <Textarea label="Einleitungstext" minRows={4} value={texts.intro} onChange={(e) => setText({ intro: e.currentTarget.value })} />
          <Textarea label="Schlusstext" minRows={4} value={texts.closing} onChange={(e) => setText({ closing: e.currentTarget.value })} />
        </Stack>
      </Card>
    </Stack>
  );
}

function LayoutsTab() {
  const navigate = useNavigate();
  const { data: layouts } = useLayouts();
  const create = useAction((base?: Layout) => api.post<Layout>('/layouts', base ? { ...base, id: 0, name: `${base.name} (Kopie)`, is_default: false } : { id: 0, name: 'Neues Layout', is_default: false, config: layouts?.[0]?.config, created_at: '', updated_at: '' }), {
    success: 'Layout erstellt',
    onSuccess: (l) => navigate(`/einstellungen/layouts/${l.id}`),
  });
  const setDefault = useAction((id: number) => api.post(`/layouts/${id}/default`), { success: 'Standard-Layout geändert' });
  const remove = useAction((id: number) => api.del(`/layouts/${id}`), { success: 'Layout gelöscht' });
  return (
    <Stack gap="lg">
      <Group justify="space-between">
        <Text size="sm" c="dimmed">
          Gestalten Sie das Erscheinungsbild Ihrer Rechnungen, Angebote und Mahnungen – mit Logo, Briefpapier, Farben und frei positionierbaren Elementen.
        </Text>
        <Button leftSection={<IconPlus size={16} />} onClick={() => create.mutate(undefined)}>
          Neues Layout
        </Button>
      </Group>
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="lg">
        {(layouts ?? []).map((l) => (
          <Card key={l.id} padding={0}>
            <Box h={180} p="md" bg="var(--app-bg)" style={{ cursor: 'pointer', borderBottom: '1px solid var(--app-border)' }} onClick={() => navigate(`/einstellungen/layouts/${l.id}`)}>
              <Paper h="100%" w={110} mx="auto" shadow="sm" radius={2} p={8} bg="white">
                {l.config.accent_bar && <Box h={4} bg={l.config.accent_color} mb={6} mx={-8} mt={-8} />}
                <Group justify="space-between" mb={10}>
                  <Box w={22} h={6} bg="#d1d5db" />
                  <Box w={30} h={10} bg={l.config.accent_color} style={{ borderRadius: 2, opacity: 0.85 }} />
                </Group>
                <Box w={40} h={4} bg="#e5e7eb" mb={3} />
                <Box w={34} h={4} bg="#e5e7eb" mb={10} />
                <Box w={50} h={6} bg="#374151" mb={8} />
                {[0, 1, 2, 3].map((i) => (
                  <Box key={i} h={4} mb={4} bg={i === 0 && l.config.table_header_fill ? l.config.accent_color : '#e5e7eb'} style={{ opacity: i === 0 ? 0.35 : 1 }} />
                ))}
                <Box w={36} h={5} ml="auto" bg="#374151" mt={6} />
              </Paper>
            </Box>
            <Group justify="space-between" p="md" wrap="nowrap" align="flex-start">
              <div style={{ minWidth: 0 }}>
                <Group gap={6} wrap="nowrap">
                  <Text fw={600} truncate>
                    {l.name}
                  </Text>
                  {l.is_default && (
                    <Badge size="sm" leftSection={<IconStar size={10} />} style={{ flexShrink: 0 }}>
                      Standard
                    </Badge>
                  )}
                </Group>
                <Text size="xs" c="dimmed">
                  {l.config.font} · {l.config.table_style}
                </Text>
              </div>
              <Group gap={4} wrap="nowrap" style={{ flexShrink: 0 }}>
                <Button size="compact-sm" variant="light" leftSection={<IconEdit size={14} />} onClick={() => navigate(`/einstellungen/layouts/${l.id}`)}>
                  Designer
                </Button>
                <Menu position="bottom-end">
                  <Menu.Target>
                    <ActionIcon aria-label="Mehr">
                      <IconDots size={16} />
                    </ActionIcon>
                  </Menu.Target>
                  <Menu.Dropdown>
                    {!l.is_default && (
                      <Menu.Item leftSection={<IconStar size={14} />} onClick={() => setDefault.mutate(l.id)}>
                        Als Standard festlegen
                      </Menu.Item>
                    )}
                    <Menu.Item leftSection={<IconCopy size={14} />} onClick={() => create.mutate(l)}>
                      Duplizieren
                    </Menu.Item>
                    <Menu.Item color="red" leftSection={<IconTrash size={14} />} onClick={() => confirmAction({ title: 'Layout löschen', message: `„${l.name}“ löschen?`, danger: true, confirmLabel: 'Löschen', onConfirm: () => remove.mutate(l.id) })}>
                      Löschen
                    </Menu.Item>
                  </Menu.Dropdown>
                </Menu>
              </Group>
            </Group>
          </Card>
        ))}
      </SimpleGrid>
    </Stack>
  );
}

function NumbersTab() {
  const { data } = useApi<NumberRange[]>('/number-ranges');
  const [rows, setRows] = useState<NumberRange[]>([]);
  useEffect(() => {
    if (data) setRows(data);
  }, [data]);
  const save = useAction(() => api.put('/number-ranges', rows), { success: 'Nummernkreise gespeichert' });
  const preview = (r: NumberRange) => {
    const d = new Date();
    let out = r.format.replace('{YYYY}', String(d.getFullYear())).replace('{YY}', String(d.getFullYear()).slice(2)).replace('{MM}', String(d.getMonth() + 1).padStart(2, '0'));
    out = out.replace(/\{(N+)\}/g, (_m, n: string) => String(r.next).padStart(n.length > 1 ? n.length : 0, '0'));
    return out;
  };
  return (
    <Card maw={1000}>
      <Text size="sm" c="dimmed" mb="md">
        Platzhalter: {'{YYYY}'} Jahr, {'{YY}'} Jahr zweistellig, {'{MM}'} Monat, {'{NNNN}'} fortlaufende Nummer (Anzahl N = Stellen). Rechnungsnummern werden lückenlos vergeben.
      </Text>
      <Table verticalSpacing="xs">
        <Table.Thead>
          <Table.Tr>
            <Table.Th>Nummernkreis</Table.Th>
            <Table.Th>Format</Table.Th>
            <Table.Th w={120}>Nächste Nr.</Table.Th>
            <Table.Th w={140}>Jährlich neu</Table.Th>
            <Table.Th>Vorschau</Table.Th>
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {rows.map((r, i) => (
            <Table.Tr key={r.key}>
              <Table.Td>
                <Text size="sm" fw={500}>
                  {r.label}
                </Text>
              </Table.Td>
              <Table.Td>
                <TextInput size="xs" value={r.format} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, format: e.currentTarget.value } : x)))} />
              </Table.Td>
              <Table.Td>
                <NumberInput size="xs" min={1} thousandSeparator="" allowDecimal={false} value={r.next} onChange={(v) => setRows(rows.map((x, j) => (j === i ? { ...x, next: Number(v) || 1 } : x)))} />
              </Table.Td>
              <Table.Td>
                <Switch size="xs" checked={r.reset_yearly} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, reset_yearly: e.currentTarget.checked } : x)))} />
              </Table.Td>
              <Table.Td>
                <Badge variant="light" color="gray" size="lg" radius="sm" tt="none">
                  {preview(r)}
                </Badge>
              </Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Group justify="flex-end" mt="md">
        <Button onClick={() => save.mutate()} loading={save.isPending} leftSection={<IconDeviceFloppy size={16} />}>
          Nummernkreise speichern
        </Button>
      </Group>
    </Card>
  );
}

function DunningTab({ s, patch }: { s: Settings; patch: Patch }) {
  const d = s.dunning;
  const set = (p: Partial<Settings['dunning']>) => patch((x) => ({ ...x, dunning: { ...x.dunning, ...p } }));
  const setLevel = (i: number, p: Partial<Settings['dunning']['levels'][number]>) => set({ levels: d.levels.map((l, j) => (j === i ? { ...l, ...p } : l)) });
  return (
    <Stack gap="lg" maw={1000}>
      {d.levels.map((l, i) => (
        <Card key={i}>
          <Group justify="space-between" mb="sm">
            <Group gap="sm">
              <Badge color={['blue', 'yellow', 'orange', 'red'][i] ?? 'red'} size="lg" variant="light">
                Stufe {l.level}
              </Badge>
              <TextInput value={l.name} onChange={(e) => setLevel(i, { name: e.currentTarget.value, title: e.currentTarget.value })} variant="unstyled" fw={650} size="md" />
            </Group>
          </Group>
          <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm" mb="sm">
            <NumberInput label={i === 0 ? 'Tage nach Fälligkeit' : 'Tage nach Ablauf der vorherigen Frist'} min={0} value={l.days} onChange={(v) => setLevel(i, { days: Number(v) || 0 })} />
            <NumberInput label="Neue Zahlungsfrist (Tage)" min={1} value={l.deadline_days} onChange={(v) => setLevel(i, { deadline_days: Number(v) || 10 })} />
            <MoneyInput label="Mahngebühr" value={l.fee} onChange={(v) => setLevel(i, { fee: v })} />
          </SimpleGrid>
          <Textarea label="Text" minRows={5} value={l.text} onChange={(e) => setLevel(i, { text: e.currentTarget.value })} description="Platzhalter: {{Anrede}}, {{Rechnungsnummer}}, {{Rechnungsdatum}}, {{Offener Betrag}}, {{Mahngebühr}}, {{Zinsen}}, {{Gesamtbetrag}}, {{Neue Frist}}, {{Firma}}" />
        </Card>
      ))}
      <Card>
        <Text fw={650} mb="sm">
          Verzugszinsen (§ 288 BGB)
        </Text>
        <Switch label="Verzugszinsen ab der 1. Mahnung berechnen" checked={d.interest_enabled} onChange={(e) => set({ interest_enabled: e.currentTarget.checked })} mb="sm" />
        <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
          <NumberInput label="Basiszinssatz" suffix=" %" decimalScale={2} decimalSeparator="," value={d.base_rate} onChange={(v) => set({ base_rate: Number(v) || 0 })} description="Aktuellen Wert bei der Bundesbank prüfen" />
          <NumberInput label="Aufschlag Unternehmen" suffix=" %-Punkte" value={d.surcharge_b2b} onChange={(v) => set({ surcharge_b2b: Number(v) || 0 })} />
          <NumberInput label="Aufschlag Verbraucher" suffix=" %-Punkte" value={d.surcharge_b2c} onChange={(v) => set({ surcharge_b2c: Number(v) || 0 })} />
        </SimpleGrid>
      </Card>
    </Stack>
  );
}

function AccountsTab() {
  const { data } = usePaymentAccounts();
  const [modal, setModal] = useState<{ open: boolean; account: PaymentAccount | null }>({ open: false, account: null });
  return (
    <Stack gap="md" maw={900}>
      <Group justify="flex-end">
        <Button leftSection={<IconPlus size={16} />} onClick={() => setModal({ open: true, account: null })}>
          Zahlungskonto hinzufügen
        </Button>
      </Group>
      {(data ?? []).map((a) => (
        <Card key={a.account.id} padding="md">
          <Group justify="space-between">
            <Group>
              <ThemeIcon variant="light" color={a.account.kind === 'cash' ? 'teal' : 'brand'}>
                {a.account.kind === 'cash' ? <IconCash size={16} /> : <IconBuildingBank size={16} />}
              </ThemeIcon>
              <div>
                <Text fw={600}>
                  {a.account.name} {a.account.archived && <Badge color="gray" size="xs">archiviert</Badge>}
                </Text>
                <Text size="xs" c="dimmed">
                  Sachkonto {a.account.ledger_account} · Anfangsbestand {money(a.account.opening_balance)} zum {date(a.account.opening_date)}
                </Text>
              </div>
            </Group>
            <Group>
              <Text fw={700} className="tabular">
                {money(a.balance)}
              </Text>
              <Button size="compact-sm" variant="subtle" onClick={() => setModal({ open: true, account: a.account })}>
                Bearbeiten
              </Button>
            </Group>
          </Group>
        </Card>
      ))}
      <PaymentAccountModal opened={modal.open} onClose={() => setModal({ open: false, account: null })} account={modal.account} />
    </Stack>
  );
}

function DataTab() {
  const navigate = useNavigate();
  const { data: status } = useStatus();
  const [resetOpen, setResetOpen] = useState(false);
  const [confirmText, setConfirmText] = useState('');
  const [restoring, setRestoring] = useState(false);
  const demo = useAction(() => api.post('/system/demo'), { success: 'Demodaten geladen' });
  const qc = useQueryClient();
  const reset = useAction(() => api.post('/system/reset', { confirm: confirmText }), {
    success: 'Alle Daten wurden gelöscht',
    invalidate: false,
    onSuccess: () => {
      setResetOpen(false);
      setConfirmText('');
      // Remove all cached data – otherwise the welcome wizard would show the deleted company.
      qc.clear();
      navigate('/willkommen', { replace: true });
    },
  });
  const restore = async (f: File | null) => {
    if (!f) return;
    setRestoring(true);
    try {
      await api.upload('/system/restore', f);
      window.location.href = '/';
    } catch (e) {
      notifyError(e, 'Wiederherstellung fehlgeschlagen');
    } finally {
      setRestoring(false);
    }
  };
  return (
    <Stack gap="lg" maw={900}>
      <Card>
        <Group gap="sm" mb="xs">
          <IconDatabase size={18} />
          <Text fw={650}>Datensicherung</Text>
        </Group>
        <Text size="sm" c="dimmed" mb="sm">
          Alle Daten liegen lokal in <code>{status?.data_dir}</code>. Erstellen Sie regelmäßig Sicherungen.
        </Text>
        <Group>
          <Button leftSection={<IconArchive size={16} />} onClick={() => downloadFile('/system/backup').catch((e) => notifyError(e))}>
            Sicherung herunterladen
          </Button>
          <FileButton onChange={(f) => f && confirmAction({ title: 'Sicherung wiederherstellen', message: 'Alle aktuellen Daten werden durch die Sicherung ersetzt.', danger: true, confirmLabel: 'Wiederherstellen', onConfirm: () => restore(f) })} accept=".zip,application/zip">
            {(props) => (
              <Button {...props} variant="default" leftSection={<IconUpload size={16} />} loading={restoring}>
                Sicherung wiederherstellen
              </Button>
            )}
          </FileButton>
        </Group>
      </Card>
      <Card>
        <Text fw={650} mb={4}>
          Demodaten
        </Text>
        <Text size="sm" c="dimmed" mb="sm">
          Lädt ein realistisches Geschäftsjahr mit Kunden, Rechnungen, Belegen, Zahlungen und Anlagen – ideal zum Ausprobieren.
        </Text>
        <Group gap="sm">
          <Button variant="light" onClick={() => demo.mutate()} loading={demo.isPending} disabled={status?.has_data} w="fit-content">
            Demodaten laden
          </Button>
          {status?.has_data && (
            <Text size="xs" c="dimmed">
              Nur bei leerem Datenbestand möglich – setzen Sie dazu vorher alle Daten zurück.
            </Text>
          )}
        </Group>
      </Card>
      <Card style={{ borderColor: 'var(--mantine-color-red-3)' }}>
        <Group gap="sm" mb={4}>
          <IconAlertTriangle size={18} color="var(--mantine-color-red-6)" />
          <Text fw={650} c="red.7">
            Alle Daten löschen
          </Text>
        </Group>
        <Text size="sm" c="dimmed" mb="sm">
          Setzt die Software vollständig zurück. Erstellen Sie vorher eine Sicherung!
        </Text>
        <Button color="red" variant="light" onClick={() => setResetOpen(true)} w="fit-content">
          Zurücksetzen …
        </Button>
      </Card>
      <Modal opened={resetOpen} onClose={() => setResetOpen(false)} title={<Text fw={650}>Alle Daten löschen</Text>}>
        <Stack>
          <Alert color="red" icon={<IconAlertTriangle size={16} />}>
            Alle Kontakte, Dokumente, Belege, Buchungen und Einstellungen werden unwiderruflich gelöscht.
          </Alert>
          <TextInput label="Zur Bestätigung LÖSCHEN eingeben" value={confirmText} onChange={(e) => setConfirmText(e.currentTarget.value)} />
          <Group justify="flex-end">
            <Button variant="default" onClick={() => setResetOpen(false)}>
              Abbrechen
            </Button>
            <Button color="red" disabled={confirmText !== 'LÖSCHEN'} onClick={() => reset.mutate()} loading={reset.isPending}>
              Endgültig löschen
            </Button>
          </Group>
        </Stack>
      </Modal>
    </Stack>
  );
}

function OverviewTab({ s, patch }: { s: Settings; patch: Patch }) {
  return (
    <Stack gap="lg" maw={760}>
      <Card>
        <Text fw={650} mb={4}>
          Startseite
        </Text>
        <Text size="sm" c="dimmed" mb="md">
          Legt fest, welche Bereiche auf der Übersicht angezeigt werden.
        </Text>
        <Switch
          label="„Erste Schritte“ anzeigen"
          description="Checkliste zur Einrichtung (Firmendaten, Logo, erster Kunde, erste Rechnung …). Sie verschwindet automatisch, sobald die wichtigsten Schritte erledigt sind."
          checked={!s.hide_getting_started}
          onChange={(e) => {
            const show = e.currentTarget.checked;
            patch((x) => ({ ...x, hide_getting_started: !show }));
          }}
        />
      </Card>
    </Stack>
  );
}

/** Public repository; shown as source code link in "Über Sqonto" once set. */
const SOURCE_URL = '';

/** Appropriate legal notices (AGPL-3.0 § 5 d): copyright, no warranty, licence. */
function AboutTab() {
  const { data: status } = useStatus();
  return (
    <Stack gap="lg" maw={760}>
      <Card>
        <Group gap="md" mb="md">
          <SqontoLogo size={48} />
          <div>
            <Text fw={700} size="lg">
              Sqonto
            </Text>
            <Text size="sm" c="dimmed">
              Version {status?.version ?? '–'} · lokale Buchhaltungs- und Rechnungssoftware
            </Text>
          </div>
        </Group>
        <Stack gap="sm">
          <Text size="sm">Copyright © 2026 Johannes Asal</Text>
          <Text size="sm">
            Sqonto ist freie Software: Sie können es unter den Bedingungen der GNU Affero General Public License, wie von der Free Software Foundation veröffentlicht, weitergeben und/oder
            verändern – entweder gemäß Version 3 der Lizenz oder (nach Ihrer Wahl) jeder späteren Version.
          </Text>
          <Text size="sm">
            Sqonto wird in der Hoffnung bereitgestellt, dass es nützlich ist, aber <b>ohne jede Gewährleistung</b> – sogar ohne die implizite Gewährleistung der Marktreife oder der Eignung für
            einen bestimmten Zweck. Die Software unterstützt bei der Buchführung, ersetzt aber keine steuerliche Beratung.
          </Text>
          <Group gap="sm" mt="xs">
            <Button component="a" href="https://www.gnu.org/licenses/agpl-3.0.html" target="_blank" rel="noreferrer" variant="default" size="xs">
              Lizenztext (AGPL-3.0)
            </Button>
            {SOURCE_URL && (
              <Button component="a" href={SOURCE_URL} target="_blank" rel="noreferrer" variant="default" size="xs">
                Quellcode
              </Button>
            )}
          </Group>
        </Stack>
      </Card>
      <Card>
        <Text fw={650} mb={4}>
          Komponenten Dritter
        </Text>
        <Text size="sm" c="dimmed">
          Sqonto enthält Bibliotheken und Schriften anderer Projekte unter deren eigenen, freien Lizenzen (u. a. MIT, Apache-2.0, BSD und SIL Open Font License). Die vollständigen Lizenztexte
          liegen in der Datei THIRD-PARTY-LICENSES.txt neben dem Programm.
        </Text>
      </Card>
    </Stack>
  );
}

const TABS = [
  { value: 'unternehmen', label: 'Unternehmen', icon: IconBuilding, saves: true },
  { value: 'steuern', label: 'Steuern & Buchführung', icon: IconReceiptTax, saves: true },
  { value: 'dokumente', label: 'Dokumente & Texte', icon: IconFileText, saves: true },
  { value: 'layouts', label: 'Layouts', icon: IconLayout, saves: false },
  { value: 'nummern', label: 'Nummernkreise', icon: IconHash, saves: false },
  { value: 'mahnwesen', label: 'Mahnwesen', icon: IconBell, saves: true },
  { value: 'konten', label: 'Zahlungskonten', icon: IconBuildingBank, saves: false },
  { value: 'uebersicht', label: 'Übersicht', icon: IconHome2, saves: true },
  { value: 'daten', label: 'Daten & Sicherung', icon: IconDatabase, saves: false },
  { value: 'info', label: 'Über Sqonto', icon: IconInfoCircle, saves: false },
];

export function SettingsPage() {
  const { tab = 'unternehmen' } = useParams();
  const navigate = useNavigate();
  const { data } = useSettings();
  const [s, setS] = useState<Settings | null>(null);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    if (data && !dirty) setS(data);
  }, [data, dirty]);
  const patch: Patch = (fn) => {
    setS((x) => (x ? fn(x) : x));
    setDirty(true);
  };
  const save = useAction((x: Settings) => api.put<Settings>('/settings', x), {
    success: 'Einstellungen gespeichert',
    onSuccess: (r) => {
      setS(r);
      setDirty(false);
    },
  });
  const current = TABS.find((t) => t.value === tab) ?? TABS[0];
  return (
    <Page>
      <PageHeader
        title="Einstellungen"
        actions={
          current.saves && (
            <Button leftSection={<IconDeviceFloppy size={16} />} onClick={() => s && save.mutate(s)} loading={save.isPending} disabled={!dirty}>
              Änderungen speichern
            </Button>
          )
        }
      />
      <Tabs value={current.value} onChange={(v) => navigate(`/einstellungen/${v}`)} orientation="horizontal">
        <Tabs.List mb="lg">
          {TABS.map((t) => (
            <Tabs.Tab key={t.value} value={t.value} leftSection={<t.icon size={16} />}>
              {t.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>
      </Tabs>
      {s && (
        <>
          {current.value === 'unternehmen' && <CompanyTab s={s} patch={patch} />}
          {current.value === 'steuern' && <TaxTab s={s} patch={patch} />}
          {current.value === 'dokumente' && <DocsTab s={s} patch={patch} />}
          {current.value === 'layouts' && <LayoutsTab />}
          {current.value === 'nummern' && <NumbersTab />}
          {current.value === 'mahnwesen' && <DunningTab s={s} patch={patch} />}
          {current.value === 'konten' && <AccountsTab />}
          {current.value === 'uebersicht' && <OverviewTab s={s} patch={patch} />}
          {current.value === 'daten' && <DataTab />}
          {current.value === 'info' && <AboutTab />}
        </>
      )}
      <UnsavedChangesGuard when={dirty} onSave={() => (s ? save.mutateAsync(s) : Promise.resolve())} allow={(path) => path === '/willkommen' || /^\/einstellungen(\/[a-z]+)?$/.test(path)} />
    </Page>
  );
}
