// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect } from 'react';
import {
  ActionIcon,
  Button,
  Checkbox,
  Divider,
  Drawer,
  Grid,
  Group,
  NumberInput,
  Paper,
  SegmentedControl,
  Select,
  Stack,
  TagsInput,
  Text,
  TextInput,
  Textarea,
} from '@mantine/core';
import { useForm } from '@mantine/form';
import { IconPlus, IconTrash, IconUserPlus } from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction } from '@/api/hooks';
import type { Contact, ContactPerson } from '@/api/types';
import { COUNTRIES, EINVOICE_FORMATS, SALUTATIONS, TAX_MODES } from '@/lib/labels';

export function emptyContact(role?: 'customer' | 'supplier'): Contact {
  return {
    id: 0,
    number: '',
    is_customer: role !== 'supplier',
    is_supplier: role === 'supplier',
    kind: 'organization',
    company: '',
    salutation: '',
    title: '',
    first_name: '',
    last_name: '',
    address_extra: '',
    street: '',
    zip: '',
    city: '',
    country: 'DE',
    email: '',
    phone: '',
    mobile: '',
    website: '',
    vat_id: '',
    tax_number: '',
    leitweg_id: '',
    iban: '',
    bic: '',
    payment_terms_days: null,
    discount_percent: null,
    einvoice_format: 'pdf',
    default_tax_mode: '',
    persons: [],
    notes: '',
    tags: [],
    archived: false,
    created_at: '',
    updated_at: '',
  };
}

const emptyPerson = (): ContactPerson => ({ salutation: '', title: '', first_name: '', last_name: '', position: '', email: '', phone: '', primary: false });

export function ContactDrawer({
  opened,
  onClose,
  contact,
  defaultRole,
  onSaved,
}: {
  opened: boolean;
  onClose: () => void;
  contact?: Contact | null;
  defaultRole?: 'customer' | 'supplier';
  onSaved?: (c: Contact) => void;
}) {
  const form = useForm<Contact>({ initialValues: contact ?? emptyContact(defaultRole) });

  useEffect(() => {
    if (opened) form.setValues(contact ?? emptyContact(defaultRole));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opened, contact?.id]);

  const save = useAction(
    (c: Contact) => (c.id ? api.put<Contact>(`/contacts/${c.id}`, c) : api.post<Contact>('/contacts', c)),
    {
      success: (c) => `Kontakt „${c.kind === 'person' ? `${c.first_name} ${c.last_name}` : c.company}“ gespeichert`,
      onSuccess: (c) => onSaved?.(c),
    },
  );

  const v = form.values;
  const isOrg = v.kind === 'organization';

  return (
    <Drawer opened={opened} onClose={onClose} title={<Text fw={650}>{v.id ? 'Kontakt bearbeiten' : 'Neuer Kontakt'}</Text>} size="xl">
      <form onSubmit={form.onSubmit((values) => save.mutate(values))}>
        <Stack gap="md">
          <Group justify="space-between">
            <SegmentedControl
              value={v.kind}
              onChange={(k) => form.setFieldValue('kind', k)}
              data={[
                { value: 'organization', label: 'Unternehmen' },
                { value: 'person', label: 'Privatperson' },
              ]}
            />
            <Group gap="lg">
              <Checkbox label="Kunde" {...form.getInputProps('is_customer', { type: 'checkbox' })} />
              <Checkbox label="Lieferant" {...form.getInputProps('is_supplier', { type: 'checkbox' })} />
            </Group>
          </Group>

          <Grid gap="sm">
            {isOrg && (
              <Grid.Col span={12}>
                <TextInput label="Firmenname" required placeholder="z. B. Muster GmbH" {...form.getInputProps('company')} data-autofocus />
              </Grid.Col>
            )}
            {!isOrg && (
              <>
                <Grid.Col span={{ base: 6, sm: 3 }}>
                  <Select label="Anrede" data={SALUTATIONS} {...form.getInputProps('salutation')} />
                </Grid.Col>
                <Grid.Col span={{ base: 6, sm: 3 }}>
                  <TextInput label="Titel" placeholder="Dr." {...form.getInputProps('title')} />
                </Grid.Col>
                <Grid.Col span={{ base: 12, sm: 3 }}>
                  <TextInput label="Vorname" {...form.getInputProps('first_name')} />
                </Grid.Col>
                <Grid.Col span={{ base: 12, sm: 3 }}>
                  <TextInput label="Nachname" required {...form.getInputProps('last_name')} />
                </Grid.Col>
              </>
            )}
            <Grid.Col span={{ base: 12, sm: 8 }}>
              <TextInput label="Straße und Hausnummer" {...form.getInputProps('street')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 4 }}>
              <TextInput label="Kundennummer" placeholder="automatisch" {...form.getInputProps('number')} />
            </Grid.Col>
            <Grid.Col span={12}>
              <TextInput label="Adresszusatz" placeholder="z. B. Abteilung, Gebäude" {...form.getInputProps('address_extra')} />
            </Grid.Col>
            <Grid.Col span={{ base: 4, sm: 3 }}>
              <TextInput label="PLZ" {...form.getInputProps('zip')} />
            </Grid.Col>
            <Grid.Col span={{ base: 8, sm: 5 }}>
              <TextInput label="Ort" {...form.getInputProps('city')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 4 }}>
              <Select label="Land" searchable data={COUNTRIES} {...form.getInputProps('country')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <TextInput label="E-Mail" type="email" {...form.getInputProps('email')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <TextInput label="Telefon" {...form.getInputProps('phone')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <TextInput label="Mobil" {...form.getInputProps('mobile')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <TextInput label="Website" {...form.getInputProps('website')} />
            </Grid.Col>
          </Grid>

          {isOrg && (
            <>
              <Divider label="Ansprechpartner" labelPosition="left" />
              <Stack gap="xs">
                {v.persons.map((p, i) => (
                  <Paper key={i} withBorder p="sm" radius="md">
                    <Grid gap="xs" align="flex-end">
                      <Grid.Col span={{ base: 6, sm: 2 }}>
                        <Select label="Anrede" data={SALUTATIONS} {...form.getInputProps(`persons.${i}.salutation`)} />
                      </Grid.Col>
                      <Grid.Col span={{ base: 6, sm: 2 }}>
                        <TextInput label="Titel" {...form.getInputProps(`persons.${i}.title`)} />
                      </Grid.Col>
                      <Grid.Col span={{ base: 6, sm: 4 }}>
                        <TextInput label="Vorname" {...form.getInputProps(`persons.${i}.first_name`)} />
                      </Grid.Col>
                      <Grid.Col span={{ base: 6, sm: 4 }}>
                        <TextInput label="Nachname" {...form.getInputProps(`persons.${i}.last_name`)} />
                      </Grid.Col>
                      <Grid.Col span={{ base: 12, sm: 4 }}>
                        <TextInput label="Position" {...form.getInputProps(`persons.${i}.position`)} />
                      </Grid.Col>
                      <Grid.Col span={{ base: 12, sm: 4 }}>
                        <TextInput label="E-Mail" {...form.getInputProps(`persons.${i}.email`)} />
                      </Grid.Col>
                      <Grid.Col span={{ base: 9, sm: 3 }}>
                        <TextInput label="Telefon" {...form.getInputProps(`persons.${i}.phone`)} />
                      </Grid.Col>
                      <Grid.Col span={{ base: 3, sm: 1 }}>
                        <ActionIcon color="red" variant="subtle" size="lg" onClick={() => form.removeListItem('persons', i)} aria-label="Ansprechpartner entfernen">
                          <IconTrash size={16} />
                        </ActionIcon>
                      </Grid.Col>
                      <Grid.Col span={12}>
                        <Checkbox
                          size="xs"
                          label="Hauptansprechpartner (erscheint in der Anschrift)"
                          checked={p.primary}
                          onChange={(e) => form.setFieldValue('persons', v.persons.map((x, j) => ({ ...x, primary: j === i ? e.currentTarget.checked : false })))}
                        />
                      </Grid.Col>
                    </Grid>
                  </Paper>
                ))}
                <Button variant="light" size="xs" leftSection={<IconUserPlus size={14} />} w="fit-content" onClick={() => form.insertListItem('persons', { ...emptyPerson(), primary: v.persons.length === 0 })}>
                  Ansprechpartner hinzufügen
                </Button>
              </Stack>
            </>
          )}

          <Divider label="Steuern, Zahlung & E-Rechnung" labelPosition="left" />
          <Grid gap="sm">
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <TextInput label="USt-IdNr." placeholder="DE123456789" {...form.getInputProps('vat_id')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <TextInput label="Steuernummer" {...form.getInputProps('tax_number')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <TextInput label="IBAN" {...form.getInputProps('iban')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <TextInput label="BIC" {...form.getInputProps('bic')} />
            </Grid.Col>
            <Grid.Col span={{ base: 6, sm: 4 }}>
              <NumberInput
                label="Zahlungsziel (Tage)"
                placeholder="Standard"
                min={0}
                max={365}
                value={v.payment_terms_days ?? ''}
                onChange={(x) => form.setFieldValue('payment_terms_days', x === '' ? null : Number(x))}
              />
            </Grid.Col>
            <Grid.Col span={{ base: 6, sm: 4 }}>
              <NumberInput
                label="Rabatt (%)"
                min={0}
                max={100}
                decimalScale={2}
                value={v.discount_percent ?? ''}
                onChange={(x) => form.setFieldValue('discount_percent', x === '' ? null : Number(x))}
              />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 4 }}>
              <TextInput label="Leitweg-ID" description="Für Behörden (XRechnung)" inputWrapperOrder={['label', 'input', 'description', 'error']} {...form.getInputProps('leitweg_id')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <Select label="Rechnungsformat" data={EINVOICE_FORMATS} {...form.getInputProps('einvoice_format')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 6 }}>
              <Select
                label="Steuerart"
                description="Leer = automatisch nach Land und USt-IdNr."
                inputWrapperOrder={['label', 'input', 'description', 'error']}
                clearable
                data={TAX_MODES}
                value={v.default_tax_mode || null}
                onChange={(x) => form.setFieldValue('default_tax_mode', x ?? '')}
                allowDeselect
              />
            </Grid.Col>
          </Grid>
          <TagsInput label="Schlagwörter" placeholder="z. B. Stammkunde, Agentur" {...form.getInputProps('tags')} />
          <Textarea label="Interne Notizen" minRows={3} {...form.getInputProps('notes')} />
          <Group justify="flex-end" mt="sm">
            <Button variant="default" onClick={onClose}>
              Abbrechen
            </Button>
            <Button type="submit" loading={save.isPending} leftSection={<IconPlus size={16} />}>
              {v.id ? 'Speichern' : 'Kontakt anlegen'}
            </Button>
          </Group>
        </Stack>
      </form>
    </Drawer>
  );
}
