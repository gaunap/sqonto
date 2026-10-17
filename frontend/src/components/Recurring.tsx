// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

// Abo-Rechnungen: schedule dialog and overview.
import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router';
import { ActionIcon, Badge, Button, Drawer, Group, Modal, Select, Stack, Switch, Table, Text } from '@mantine/core';
import { IconEdit, IconExternalLink, IconTrash } from '@tabler/icons-react';
import dayjs from 'dayjs';
import { api } from '@/api/client';
import { useAction, useApi } from '@/api/hooks';
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

/** Creates or edits the schedule of an invoice template. */
export function RecurringModal({
  opened,
  onClose,
  templateId,
  templateDate,
  existing,
}: {
  opened: boolean;
  onClose: () => void;
  templateId: number;
  templateDate?: string;
  existing?: RecurringInvoice | null;
}) {
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
      },
    );
  }, [opened, existing, templateId, templateDate]);
  const save = useAction((x: RecurringInvoice) => (x.id ? api.put(`/recurring/${x.id}`, x) : api.post('/recurring', x)), {
    success: 'Abo-Rechnung gespeichert',
    onSuccess: onClose,
  });
  if (!r) return null;
  const set = (p: Partial<RecurringInvoice>) => setR({ ...r, ...p });
  return (
    <Modal opened={opened} onClose={onClose} title={<Text fw={650}>{r.id ? 'Abo-Rechnung bearbeiten' : 'Als Abo-Rechnung wiederholen'}</Text>}>
      <Stack gap="sm">
        <Text size="sm" c="dimmed">
          Zu jedem Termin entsteht ein Rechnungsentwurf mit denselben Positionen und Texten, dem Rechnungsdatum als Leistungsbeginn und neuem Fälligkeitsdatum. Sie prüfen den Entwurf und schließen ihn ab.
        </Text>
        <Select label="Rhythmus" data={CYCLES.map(({ value, label }) => ({ value, label }))} value={r.cycle} onChange={(v) => set({ cycle: v ?? 'monthly', day: 0 })} />
        <DateField label="Nächste Rechnung am" value={r.next_date} onChange={(v) => v && set({ next_date: v, day: 0 })} required />
        <DateField label="Endet nach dem" value={r.end_date} onChange={(v) => set({ end_date: v })} clearable placeholder="unbefristet" />
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
                    {v.title} · {money(v.gross_total)} · Vorlage {v.template_number || 'Entwurf'}
                  </Text>
                </Table.Td>
                <Table.Td>
                  <Badge variant="light">{cycleLabel(v.recurring.cycle)}</Badge>
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
      {editing && <RecurringModal opened onClose={() => setEditing(null)} templateId={editing.recurring.template_id} existing={editing.recurring} />}
    </Drawer>
  );
}
