// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// Abo-Rechnungen: schedule dialog and overview.
import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router';
import { ActionIcon, Alert, Badge, Button, Checkbox, Collapse, Drawer, Group, Modal, NumberInput, Select, SimpleGrid, Stack, Switch, Table, Text, Tooltip } from '@mantine/core';
import { IconAlertTriangle, IconEdit, IconExternalLink, IconMail, IconTrash } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import { useAction, useApi, useSettings } from '@/api/hooks';
import type { RecurringInvoice, RecurringView } from '@/api/types';
import { DateField } from '@/components/inputs';
import { confirmAction } from '@/components/ui';
import { date, money } from '@/lib/format';

export const CYCLES = [
  { value: 'monthly', label: 'Monatlich', months: 1 },
  { value: 'quarterly', label: 'Vierteljährlich', months: 3 },
  { value: 'halfyearly', label: 'Halbjährlich', months: 6 },
  { value: 'yearly', label: 'Jährlich', months: 12 },
];

const cycleLabel = (c: string) => CYCLES.find((x) => x.value === c)?.label ?? c;

const percent = (p: number) => `${p > 0 ? '+' : ''}${p.toLocaleString('de-DE', { maximumFractionDigits: 2 })} %`;

/** What happens with the invoices of an Abo-Rechnung, for short summaries. */
export function recurringMode(r: RecurringInvoice) {
  if (r.auto_send) return 'abschließen und per E-Mail versenden';
  if (r.auto_finalize) return 'automatisch abschließen';
  return 'Entwurf zur Prüfung';
}

/** Creates or edits the schedule of an invoice template. */
export function RecurringModal({
  opened,
  onClose,
  templateId,
  templateDate,
  existing,
  contactEmail,
}: {
  opened: boolean;
  onClose: () => void;
  templateId: number;
  templateDate?: string;
  existing?: RecurringInvoice | null;
  /** E-mail address of the customer, for the automatic sending. */
  contactEmail?: string;
}) {
  const { data: settings } = useSettings();
  const [r, setR] = useState<RecurringInvoice | null>(null);
  useEffect(() => {
    if (!opened) return;
    setR(
      existing ?? {
        id: 0,
        template_id: templateId,
        cycle: 'monthly',
        day: 0,
        next_date: dayjs(templateDate).add(1, 'month').format('YYYY-MM-DD'),
        end_date: null,
        active: true,
        last_document_id: null,
        created_at: '',
        auto_finalize: false,
        auto_send: false,
        price_change_percent: 0,
        price_change_from: null,
        price_change_yearly: false,
        last_error: '',
      },
    );
  }, [opened, existing, templateId, templateDate]);
  const save = useAction((x: RecurringInvoice) => (x.id ? api.put(`/recurring/${x.id}`, x) : api.post('/recurring', x)), {
    success: 'Abo-Rechnung gespeichert',
    onSuccess: onClose,
  });
  if (!r) return null;
  const set = (p: Partial<RecurringInvoice>) => setR({ ...r, ...p });
  const smtp = settings?.email.enabled ?? false;
  const mode = r.auto_send ? 'send' : r.auto_finalize ? 'finalize' : 'draft';
  const priceChange = r.price_change_percent !== 0 || r.price_change_from !== null;
  return (
    <Modal opened={opened} onClose={onClose} size="lg" title={<Text fw={650}>{r.id ? 'Abo-Rechnung bearbeiten' : 'Als Abo-Rechnung wiederholen'}</Text>}>
      <Stack gap="sm">
        <Text size="sm" c="dimmed">
          Zu jedem Termin entsteht eine Rechnung mit denselben Positionen und Texten, dem Rechnungsdatum als Leistungsbeginn und neuem Fälligkeitsdatum. Platzhalter wie {'{{Monat}}'}, {'{{Quartal}}'} oder {'{{Leistungszeitraum}}'} in Titel und
          Positionen werden für jeden Zeitraum ausgefüllt.
        </Text>
        <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm">
          <Select label="Rhythmus" data={CYCLES.map(({ value, label }) => ({ value, label }))} value={r.cycle} onChange={(v) => set({ cycle: v ?? 'monthly', day: 0 })} allowDeselect={false} />
          <DateField label="Nächste Rechnung am" value={r.next_date} onChange={(v) => v && set({ next_date: v, day: 0 })} required />
          <DateField label="Endet nach dem" value={r.end_date} onChange={(v) => set({ end_date: v })} clearable placeholder="unbefristet" />
        </SimpleGrid>
        <Select
          label="Neue Rechnungen"
          allowDeselect={false}
          value={mode}
          onChange={(v) => set({ auto_finalize: v !== 'draft', auto_send: v === 'send' })}
          data={[
            { value: 'draft', label: 'Als Entwurf anlegen – ich prüfe und schließe selbst ab' },
            { value: 'finalize', label: 'Automatisch abschließen' },
            { value: 'send', label: 'Automatisch abschließen und per E-Mail versenden', disabled: !smtp || !contactEmail },
          ]}
          description={
            !smtp
              ? 'Für den automatischen Versand zuerst den E-Mail-Versand einrichten (Einstellungen → E-Mail).'
              : !contactEmail
                ? 'Für den automatischen Versand fehlt die E-Mail-Adresse des Kunden.'
                : mode === 'send'
                  ? `Versand an ${contactEmail} mit der E-Mail-Vorlage aus den Einstellungen.`
                  : mode === 'finalize'
                    ? 'Die Rechnungen erhalten sofort eine Nummer und können nicht mehr geändert werden.'
                    : undefined
          }
        />
        <Checkbox
          label="Preisanpassung"
          description="z. B. jährliche Erhöhung laut Vertrag; die Vorlage bleibt unverändert"
          checked={priceChange}
          onChange={(e) => set(e.currentTarget.checked ? { price_change_percent: 3, price_change_from: dayjs(r.next_date).add(1, 'year').startOf('year').format('YYYY-MM-DD'), price_change_yearly: true } : { price_change_percent: 0, price_change_from: null, price_change_yearly: false })}
        />
        <Collapse expanded={priceChange}>
          <SimpleGrid cols={{ base: 1, sm: 3 }} spacing="sm" pl={32}>
            <NumberInput label="Preise ändern um" suffix=" %" decimalScale={2} decimalSeparator="," min={-50} max={100} value={r.price_change_percent} onChange={(v) => set({ price_change_percent: Number(v) || 0 })} />
            <DateField label="ab der Rechnung vom" value={r.price_change_from} onChange={(v) => set({ price_change_from: v })} />
            <Switch mt={30} label="jedes Jahr erneut" checked={r.price_change_yearly} onChange={(e) => set({ price_change_yearly: e.currentTarget.checked })} />
          </SimpleGrid>
        </Collapse>
        {r.id > 0 && <Switch label="Aktiv" checked={r.active} onChange={(e) => set({ active: e.currentTarget.checked })} />}
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            Abbrechen
          </Button>
          <Button onClick={() => save.mutate(r)} loading={save.isPending}>
            Speichern
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** Problem of the last automatic run of an Abo-Rechnung. */
export function RecurringError({ message }: { message: string }) {
  if (!message) return null;
  return (
    <Alert color="orange" icon={<IconAlertTriangle size={16} />} p="xs" mt={6}>
      <Text size="xs">{message}</Text>
    </Alert>
  );
}

/** Overview of all Abo-Rechnungen. */
export function RecurringDrawer({ opened, onClose }: { opened: boolean; onClose: () => void }) {
  const navigate = useNavigate();
  const { data } = useApi<RecurringView[]>(opened ? '/recurring' : null);
  const [editing, setEditing] = useState<RecurringView | null>(null);
  const toggle = useAction((r: RecurringInvoice) => api.put(`/recurring/${r.id}`, { ...r, active: !r.active }));
  const remove = useAction((id: number) => api.del(`/recurring/${id}`), { success: 'Abo-Rechnung gelöscht' });
  return (
    <Drawer opened={opened} onClose={onClose} size="xl" title={<Text fw={650}>Abo-Rechnungen</Text>}>
      {!data?.length ? (
        <Text size="sm" c="dimmed">
          Noch keine Abo-Rechnungen. Öffnen Sie eine Rechnung und wählen Sie „Als Abo-Rechnung wiederholen“.
        </Text>
      ) : (
        <Table verticalSpacing="sm">
          <Table.Tbody>
            {data.map((v) => (
              <Table.Tr key={v.recurring.id} style={{ opacity: v.recurring.active ? 1 : 0.55 }}>
                <Table.Td>
                  <Text size="sm" fw={600}>
                    {v.contact_name}
                  </Text>
                  <Text size="xs" c="dimmed">
                    {v.title} · {money(v.next_gross_total)}
                    {Math.abs(v.next_gross_total - v.gross_total) >= 0.005 && ` (Vorlage ${money(v.gross_total)})`} · Vorlage {v.template_number || 'Entwurf'}
                  </Text>
                  <RecurringError message={v.recurring.last_error} />
                </Table.Td>
                <Table.Td>
                  <Group gap={4}>
                    <Badge variant="light">{cycleLabel(v.recurring.cycle)}</Badge>
                    {v.recurring.auto_send ? (
                      <Tooltip label={`Wird abgeschlossen und an ${v.contact_email} gesendet`}>
                        <Badge variant="light" color="teal" leftSection={<IconMail size={11} />}>
                          Versand
                        </Badge>
                      </Tooltip>
                    ) : (
                      v.recurring.auto_finalize && (
                        <Badge variant="light" color="teal">
                          automatisch
                        </Badge>
                      )
                    )}
                    {v.recurring.price_change_percent !== 0 && v.recurring.price_change_from && (
                      <Tooltip label={`ab ${date(v.recurring.price_change_from)}${v.recurring.price_change_yearly ? ', jedes Jahr erneut' : ''}`}>
                        <Badge variant="light" color="grape">
                          {percent(v.recurring.price_change_percent)}
                        </Badge>
                      </Tooltip>
                    )}
                  </Group>
                  <Text size="xs" c="dimmed" mt={4}>
                    {v.recurring.active ? `nächste am ${date(v.recurring.next_date)}` : 'beendet'}
                    {v.recurring.end_date ? ` · bis ${date(v.recurring.end_date)}` : ''}
                    {v.last_number ? ` · zuletzt ${v.last_number}` : ''}
                  </Text>
                </Table.Td>
                <Table.Td w={160}>
                  <Group gap={4} justify="flex-end" wrap="nowrap">
                    <Switch size="xs" checked={v.recurring.active} onChange={() => toggle.mutate(v.recurring)} aria-label="Aktiv" />
                    <ActionIcon variant="subtle" onClick={() => setEditing(v)} aria-label="Bearbeiten">
                      <IconEdit size={16} />
                    </ActionIcon>
                    <ActionIcon variant="subtle" onClick={() => navigate(`/dokumente/${v.recurring.template_id}`)} aria-label="Vorlage öffnen">
                      <IconExternalLink size={16} />
                    </ActionIcon>
                    <ActionIcon
                      variant="subtle"
                      color="red"
                      onClick={() => confirmAction({ title: 'Abo-Rechnung löschen', message: `Für ${v.contact_name} werden keine weiteren Rechnungen erstellt. Bereits erstellte Rechnungen bleiben erhalten.`, danger: true, confirmLabel: 'Löschen', onConfirm: () => remove.mutate(v.recurring.id) })}
                      aria-label="Löschen"
                    >
                      <IconTrash size={16} />
                    </ActionIcon>
                  </Group>
                </Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      )}
      {editing && <RecurringModal opened onClose={() => setEditing(null)} templateId={editing.recurring.template_id} existing={editing.recurring} contactEmail={editing.contact_email} />}
    </Drawer>
  );
}
