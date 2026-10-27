// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// SEPA-Lastschriftmandate of a customer.
import { useEffect, useState } from 'react';
import { ActionIcon, Alert, Badge, Button, Group, Menu, Modal, Select, SimpleGrid, Stack, Switch, Text, TextInput, Textarea, Tooltip } from '@mantine/core';
import { IconBan, IconDots, IconEdit, IconFileText, IconInfoCircle, IconPlus, IconTrash } from '@tabler/icons-react';
import { api, openFile } from '@/api/client';
import { useAction, useApi, useSettings } from '@/api/hooks';
import type { Contact, Mandate, MandateView, ReferenceSuggestion } from '@/api/types';
import { DateField, displayName } from '@/components/inputs';
import { SectionCard, confirmAction } from '@/components/ui';
import { date, formatIban, today } from '@/lib/format';
import { SCHEMES, creditorIdValid, ibanValid } from '@/lib/sepa';

const STATUS_COLORS: Record<string, string> = { active: 'teal', revoked: 'red', expired: 'orange', used: 'gray' };

function MandateModal({ contact, existing, onClose }: { contact: Contact; existing: Mandate | null; onClose: () => void }) {
  const [m, setM] = useState<Mandate | null>(null);
  useEffect(() => {
    if (existing) {
      setM(existing);
      return;
    }
    api
      .get<ReferenceSuggestion>('/mandates/reference', { contact_id: contact.id })
      .then((r) =>
        setM({
          id: 0,
          contact_id: contact.id,
          reference: r.reference,
          signed_on: today(),
          scheme: 'CORE',
          recurring: true,
          account_holder: displayName(contact),
          iban: contact.iban,
          bic: contact.bic,
          revoked_on: null,
          notes: '',
          created_at: '',
        }),
      )
      .catch(() => setM(null));
  }, [existing, contact]);
  const save = useAction((x: Mandate) => (x.id ? api.put<MandateView>(`/mandates/${x.id}`, x) : api.post<MandateView>('/mandates', x)), {
    success: (v) => `Mandat ${v.mandate.reference} gespeichert`,
    onSuccess: onClose,
  });
  if (!m) return null;
  const set = (p: Partial<Mandate>) => setM({ ...m, ...p });
  return (
    <Modal opened onClose={onClose} size="lg" title={<Text fw={650}>{m.id ? 'Lastschriftmandat bearbeiten' : 'Lastschriftmandat erfassen'}</Text>}>
      <Stack gap="sm">
        {!m.id && (
          <Alert variant="light" color="gray" icon={<IconInfoCircle size={16} />}>
            <Text size="sm">Das Mandat muss vom Kontoinhaber unterschrieben vorliegen. Nach dem Speichern lässt sich ein vorausgefülltes Formular zum Unterschreiben drucken.</Text>
          </Alert>
        )}
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
          <TextInput label="Mandatsreferenz" required maxLength={35} value={m.reference} onChange={(e) => set({ reference: e.currentTarget.value })} />
          <DateField label="Unterschrieben am" required value={m.signed_on} onChange={(v) => v && set({ signed_on: v })} maxDate={new Date()} />
          <Select label="Art" data={SCHEMES} value={m.scheme} onChange={(v) => set({ scheme: v ?? 'CORE' })} allowDeselect={false} description={m.scheme === 'B2B' ? 'Nur für Konten von Unternehmen; kein Erstattungsanspruch, die Bank des Kunden muss das Mandat kennen' : 'Erstattung innerhalb von 8 Wochen möglich'} />
          <Switch mt={28} label="Wiederkehrende Lastschriften" description={m.recurring ? undefined : 'Einmal-Mandat für eine einzige Lastschrift'} checked={m.recurring} onChange={(e) => set({ recurring: e.currentTarget.checked })} />
          <TextInput label="Kontoinhaber" value={m.account_holder} onChange={(e) => set({ account_holder: e.currentTarget.value })} />
          <TextInput label="IBAN" required value={m.iban} onChange={(e) => set({ iban: e.currentTarget.value })} error={m.iban.trim() && !ibanValid(m.iban) ? 'IBAN ungültig' : undefined} />
          <TextInput label="BIC" description="optional" value={m.bic} onChange={(e) => set({ bic: e.currentTarget.value })} />
          {m.id > 0 && <DateField label="Widerrufen zum" value={m.revoked_on} onChange={(v) => set({ revoked_on: v })} clearable placeholder="nicht widerrufen" />}
        </SimpleGrid>
        <Textarea label="Notiz" value={m.notes} onChange={(e) => set({ notes: e.currentTarget.value })} autosize minRows={2} />
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button onClick={() => save.mutate(m)} loading={save.isPending}>
            Speichern
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** Mandates of a customer with status, form and revocation. */
export function MandatesCard({ contact }: { contact: Contact }) {
  const { data } = useApi<MandateView[]>('/mandates', { contact_id: contact.id });
  const { data: settings } = useSettings();
  const [editing, setEditing] = useState<Mandate | null | 'new'>(null);
  const revoke = useAction((m: Mandate) => api.put(`/mandates/${m.id}`, { ...m, revoked_on: today() }), { success: 'Mandat widerrufen' });
  const remove = useAction((id: number) => api.del(`/mandates/${id}`), { success: 'Mandat gelöscht' });
  const creditorMissing = settings && !creditorIdValid(settings.debit.creditor_id);
  return (
    <SectionCard
      title="SEPA-Lastschriftmandate"
      actions={
        <Button size="compact-sm" variant="light" leftSection={<IconPlus size={14} />} onClick={() => setEditing('new')}>
          Mandat
        </Button>
      }
    >
      {!data?.length ? (
        <Text size="sm" c="dimmed">
          Mit einem Mandat werden Rechnungen an diesen Kunden per Lastschrift eingezogen.
        </Text>
      ) : (
        <Stack gap="sm">
          {data.map((v) => (
            <Group key={v.mandate.id} justify="space-between" wrap="nowrap" align="flex-start" style={{ opacity: v.usable ? 1 : 0.6 }}>
              <div style={{ minWidth: 0 }}>
                <Group gap={6}>
                  <Text size="sm" fw={600}>
                    {v.mandate.reference}
                  </Text>
                  <Tooltip label={v.expires_on ? `verfällt am ${date(v.expires_on)}, wenn bis dahin nicht eingezogen wird` : v.status_label}>
                    <Badge size="xs" variant="light" color={STATUS_COLORS[v.status]}>
                      {v.status === 'active' ? 'aktiv' : v.status_label.split(' ')[0]}
                    </Badge>
                  </Tooltip>
                </Group>
                <Text size="xs" c="dimmed">
                  {v.mandate.scheme === 'B2B' ? 'Firmenlastschrift' : 'Basislastschrift'} · {v.mandate.recurring ? 'wiederkehrend' : 'einmalig'} · vom {date(v.mandate.signed_on)}
                </Text>
                <Text size="xs" c="dimmed" ff="monospace">
                  {formatIban(v.mandate.iban)}
                </Text>
                <Text size="xs" c="dimmed">
                  {v.collections ? `${v.collections} Lastschrift${v.collections === 1 ? '' : 'en'}, zuletzt ${date(v.last_collection)}` : 'noch nicht genutzt'}
                  {v.status !== 'active' && ` · ${v.status_label}`}
                </Text>
              </div>
              <Menu position="bottom-end">
                <Menu.Target>
                  <ActionIcon variant="subtle" aria-label="Aktionen">
                    <IconDots size={16} />
                  </ActionIcon>
                </Menu.Target>
                <Menu.Dropdown>
                  <Menu.Item leftSection={<IconFileText size={16} />} onClick={() => openFile(`/mandates/${v.mandate.id}/pdf`)}>
                    Formular zum Unterschreiben
                  </Menu.Item>
                  <Menu.Item leftSection={<IconEdit size={16} />} onClick={() => setEditing(v.mandate)}>
                    Bearbeiten
                  </Menu.Item>
                  {!v.mandate.revoked_on && (
                    <Menu.Item
                      leftSection={<IconBan size={16} />}
                      onClick={() =>
                        confirmAction({
                          title: 'Mandat widerrufen',
                          message: `Mit dem Mandat ${v.mandate.reference} wird ab heute nicht mehr eingezogen. Offene Rechnungen mit diesem Mandat müssen überwiesen werden.`,
                          confirmLabel: 'Widerrufen',
                          danger: true,
                          onConfirm: () => revoke.mutate(v.mandate),
                        })
                      }
                    >
                      Widerrufen
                    </Menu.Item>
                  )}
                  {v.collections === 0 && (
                    <Menu.Item color="red" leftSection={<IconTrash size={16} />} onClick={() => confirmAction({ title: 'Mandat löschen', message: 'Das Mandat wird gelöscht.', confirmLabel: 'Löschen', danger: true, onConfirm: () => remove.mutate(v.mandate.id) })}>
                      Löschen
                    </Menu.Item>
                  )}
                </Menu.Dropdown>
              </Menu>
            </Group>
          ))}
        </Stack>
      )}
      {creditorMissing && (data?.length ?? 0) > 0 && (
        <Alert color="orange" mt="sm" p="xs">
          <Text size="xs">Für den Einzug fehlt noch Ihre Gläubiger-ID (Einstellungen → Unternehmen).</Text>
        </Alert>
      )}
      {editing && <MandateModal contact={contact} existing={editing === 'new' ? null : editing} onClose={() => setEditing(null)} />}
    </SectionCard>
  );
}
