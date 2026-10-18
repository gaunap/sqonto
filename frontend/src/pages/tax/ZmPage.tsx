// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useState } from 'react';
import { Link } from 'react-router';
import { Alert, Anchor, Badge, Box, Button, Card, Grid, Group, Modal, Paper, SimpleGrid, Stack, Table, Text, TextInput, ThemeIcon, UnstyledButton } from '@mantine/core';
import { IconAlertTriangle, IconCheck, IconDownload, IconFileTypeCsv, IconInfoCircle, IconSend, IconWorld, IconX } from '@tabler/icons-react';
import { api, downloadFile } from '@/api/client';
import { notifyError, useAction, useApi, useSettings } from '@/api/hooks';
import type { ZmDocument, ZmPeriod, ZmReport } from '@/api/types';
import { DateField, YearSelect } from '@/components/inputs';
import { Page, PageHeader, confirmAction } from '@/components/ui';
import { date, money, num, relativeDays, today } from '@/lib/format';

const KIND_LABEL: Record<string, string> = { L: 'Innergemeinschaftliche Lieferung', S: 'Sonstige Leistung' };

const euros = (v: number) => `${num(v, 0)} €`;

function PeriodCard({ p, active, onClick }: { p: ZmPeriod; active: boolean; onClick: () => void }) {
  const empty = p.rows === 0 && p.issues === 0;
  const state = p.changed ? 'changed' : p.filing ? 'filed' : p.is_future ? 'future' : p.is_current ? 'current' : empty ? 'empty' : 'due';
  const color = { changed: 'red', filed: 'teal', future: 'gray', current: 'blue', empty: 'gray', due: 'orange' }[state];
  const label = { changed: 'geändert', filed: 'gemeldet', future: 'zukünftig', current: 'laufend', empty: 'keine Meldung', due: 'offen' }[state];
  return (
    <UnstyledButton onClick={onClick}>
      <Paper withBorder p="sm" radius="md" style={{ outline: active ? '2px solid var(--mantine-color-brand-5)' : undefined, opacity: p.is_future ? 0.6 : 1 }}>
        <Group justify="space-between" mb={4} wrap="nowrap">
          <Text size="sm" fw={600}>
            {p.label.replace(/ \d{4}$/, '')}
          </Text>
          <Badge size="xs" color={color} variant="light">
            {label}
          </Badge>
        </Group>
        <Text fw={700} className="tabular" c={empty ? 'dimmed' : undefined}>
          {euros(p.total)}
        </Text>
        <Text size="xs" c="dimmed">
          fällig {date(p.due_date)}
        </Text>
      </Paper>
    </UnstyledButton>
  );
}

function DocLinks({ docs }: { docs: ZmDocument[] }) {
  return (
    <Text size="xs" c="dimmed">
      {docs.map((d, i) => (
        <span key={d.id}>
          {i > 0 && ', '}
          <Anchor component={Link} to={`/dokumente/${d.id}`} size="xs">
            {d.number}
          </Anchor>
          {d.amount < 0 && ` (${money(d.amount)})`}
        </span>
      ))}
    </Text>
  );
}

export function ZmPage() {
  const { data: settings } = useSettings();
  const [year, setYear] = useState(new Date().getFullYear());
  const { data: periods } = useApi<ZmPeriod[]>('/reports/zm', { year });
  const [period, setPeriod] = useState<string | null>(null);
  const [fileOpen, setFileOpen] = useState(false);
  const [fileDate, setFileDate] = useState<string | null>(today());
  const [note, setNote] = useState('Über ELSTER übermittelt');

  useEffect(() => {
    if (periods && (!period || !periods.some((p) => p.period === period))) {
      const current = periods.find((p) => p.is_current) ?? periods.filter((p) => !p.is_future).at(-1) ?? periods[0];
      setPeriod(current?.period ?? null);
    }
  }, [periods, period]);

  const { data: report } = useApi<ZmReport>(period ? `/reports/zm/${year}/${period}` : null);
  const file = useAction(() => api.post(`/reports/zm/${year}/${period}/filing`, { submitted_at: fileDate, note }), {
    success: 'Als übermittelt markiert',
    onSuccess: () => setFileOpen(false),
  });
  const unfile = useAction(() => api.del(`/reports/zm/${year}/${period}/filing`), { success: 'Meldung zurückgesetzt' });
  const download = (kind: 'csv' | 'pdf') =>
    (kind === 'csv' ? downloadFile(`/reports/zm/${year}/${period}/csv`) : downloadFile('/reports/pdf/zm', { year, period: period ?? undefined })).catch((e) => notifyError(e));

  return (
    <Page>
      <PageHeader
        title="Zusammenfassende Meldung"
        subtitle={settings ? `${settings.tax.zm_period === 'monthly' ? 'Monatlich' : 'Vierteljährlich'} · innergemeinschaftliche Lieferungen und sonstige Leistungen (§ 18a UStG)` : undefined}
        actions={<YearSelect value={year} onChange={setYear} />}
      />
      {settings?.tax.small_business && (
        <Alert color="blue" icon={<IconInfoCircle size={16} />} mb="lg" title="Kleinunternehmerregelung">
          Kleinunternehmer nach § 19 UStG müssen keine Zusammenfassende Meldung abgeben.
        </Alert>
      )}
      {report?.monthly_required && (
        <Alert color="orange" icon={<IconAlertTriangle size={16} />} mb="lg" title="Monatliche Abgabe erforderlich">
          Die innergemeinschaftlichen Lieferungen übersteigen in diesem oder einem der vier vorangegangenen Quartale 50.000 €. Die Zusammenfassende Meldung ist dann monatlich abzugeben (Einstellungen → Steuern).
        </Alert>
      )}
      <SimpleGrid cols={{ base: 2, sm: periods && periods.length > 4 ? 6 : 4 }} spacing="sm" mb="lg">
        {(periods ?? []).map((p) => (
          <PeriodCard key={p.period} p={p} active={p.period === period} onClick={() => setPeriod(p.period)} />
        ))}
      </SimpleGrid>
      {report && (
        <Grid gap="lg">
          <Grid.Col span={{ base: 12, lg: 8 }}>
            <Stack gap="lg">
              {report.issues.length > 0 && (
                <Alert color="red" icon={<IconAlertTriangle size={16} />} title="Nicht gemeldete Rechnungen">
                  <Text size="sm" mb="xs">
                    Diese Belege fehlen in der Meldung. Bitte die USt-IdNr. beim Kunden ergänzen bzw. korrigieren.
                  </Text>
                  <Stack gap={4}>
                    {report.issues.map((i) => (
                      <Text size="sm" key={i.document.id}>
                        <Anchor component={Link} to={`/dokumente/${i.document.id}`} size="sm">
                          {i.document.number}
                        </Anchor>{' '}
                        · {i.document.contact_name} · {money(i.document.amount)} – {i.message}
                      </Text>
                    ))}
                  </Stack>
                </Alert>
              )}
              <Card padding={0}>
                <Box p="md">
                  <Text fw={650} size="lg">
                    {report.period_label}
                  </Text>
                  <Text size="sm" c="dimmed">
                    {date(report.from)} – {date(report.to)}
                  </Text>
                </Box>
                {report.rows.length === 0 ? (
                  <Text c="dimmed" size="sm" px="md" pb="md">
                    {report.issues.length > 0
                      ? 'Noch keine meldefähigen Umsätze – bitte zuerst die Hinweise oben klären.'
                      : 'In diesem Zeitraum gibt es keine innergemeinschaftlichen Umsätze. Eine Meldung ist nur abzugeben, wenn etwas zu melden ist.'}
                  </Text>
                ) : (
                  <Table verticalSpacing="xs" horizontalSpacing="sm">
                    <Table.Thead bg="var(--app-bg)">
                      <Table.Tr>
                        <Table.Th>USt-IdNr.</Table.Th>
                        <Table.Th>Kunde</Table.Th>
                        <Table.Th>Art der Leistung</Table.Th>
                        <Table.Th ta="right">Summe</Table.Th>
                      </Table.Tr>
                    </Table.Thead>
                    <Table.Tbody>
                      {report.rows.map((r) => (
                        <Table.Tr key={r.vat_id + r.kind}>
                          <Table.Td>
                            <Text size="sm" ff="monospace">
                              {r.vat_id}
                            </Text>
                          </Table.Td>
                          <Table.Td>
                            <Text size="sm">{r.contact_name}</Text>
                            <DocLinks docs={r.documents} />
                          </Table.Td>
                          <Table.Td>
                            <Badge variant="light" color={r.kind === 'L' ? 'brand' : 'grape'} size="sm">
                              {r.kind}
                            </Badge>{' '}
                            <Text span size="sm">
                              {KIND_LABEL[r.kind]}
                            </Text>
                          </Table.Td>
                          <Table.Td ta="right" className="tabular">
                            <Text size="sm" fw={500} c={r.reported < 0 ? 'red.7' : undefined}>
                              {euros(r.reported)}
                            </Text>
                          </Table.Td>
                        </Table.Tr>
                      ))}
                    </Table.Tbody>
                  </Table>
                )}
                <Text size="xs" c="dimmed" p="md">
                  Gemeldet werden die Nettobeträge in vollen Euro. Maßgeblich ist bei Lieferungen das Rechnungsdatum (spätestens der Monat nach der Lieferung), bei sonstigen Leistungen das Ende der Leistung. Gutschriften und Stornos mindern den Zeitraum, in dem sie erstellt wurden; Abschlagsrechnungen werden erst mit der Schlussrechnung gemeldet.
                </Text>
              </Card>
            </Stack>
          </Grid.Col>
          <Grid.Col span={{ base: 12, lg: 4 }}>
            <Stack gap="lg">
              <Card>
                <Group justify="space-between" align="flex-start">
                  <Box>
                    <Text size="sm" c="dimmed">
                      Zu melden
                    </Text>
                    <Text fz={30} fw={700} className="tabular">
                      {euros(report.total_deliveries + report.total_services)}
                    </Text>
                    <Text size="sm" c="dimmed">
                      fällig am {date(report.due_date)} ({relativeDays(report.due_date)})
                    </Text>
                  </Box>
                  <ThemeIcon size={44} radius="xl" variant="light">
                    <IconWorld size={22} />
                  </ThemeIcon>
                </Group>
                <SimpleGrid cols={2} mt="md">
                  <Box>
                    <Text size="xs" c="dimmed">
                      Lieferungen (L)
                    </Text>
                    <Text fw={600} className="tabular">
                      {euros(report.total_deliveries)}
                    </Text>
                  </Box>
                  <Box>
                    <Text size="xs" c="dimmed">
                      Sonstige Leistungen (S)
                    </Text>
                    <Text fw={600} className="tabular">
                      {euros(report.total_services)}
                    </Text>
                  </Box>
                </SimpleGrid>
                <Stack gap="xs" mt="lg">
                  {report.filing ? (
                    <Alert color="teal" icon={<IconCheck size={16} />} title="Übermittelt">
                      am {date(report.filing.submitted_at)} · {euros(report.filing.amount)}
                      {report.filing.note && <Text size="xs">{report.filing.note}</Text>}
                      {report.changed && (
                        <Text size="xs" c="orange.8" mt={4}>
                          Achtung: Die Meldezeilen haben sich seit der Meldung geändert. Bitte eine berichtigte Zusammenfassende Meldung abgeben.
                        </Text>
                      )}
                    </Alert>
                  ) : (
                    <Button leftSection={<IconSend size={16} />} onClick={() => setFileOpen(true)} disabled={new Date(report.to) > new Date() || report.rows.length === 0}>
                      Als übermittelt markieren
                    </Button>
                  )}
                  {report.filing && (
                    <Button
                      variant="subtle"
                      color="gray"
                      leftSection={<IconX size={16} />}
                      onClick={() =>
                        confirmAction({
                          title: 'Meldung zurücksetzen',
                          message: `Die Markierung „übermittelt“ für ${report.period_label} wird entfernt. Eine bereits übermittelte Meldung bleibt davon unberührt.`,
                          confirmLabel: 'Zurücksetzen',
                          onConfirm: () => unfile.mutate(),
                        })
                      }
                    >
                      Meldung zurücksetzen
                    </Button>
                  )}
                  <Button variant="default" leftSection={<IconFileTypeCsv size={16} />} onClick={() => download('csv')} disabled={report.rows.length === 0}>
                    CSV-Datei für ELSTER
                  </Button>
                  <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => download('pdf')}>
                    Ausdruck (PDF)
                  </Button>
                </Stack>
              </Card>
              <Alert variant="light" color="gray" icon={<IconInfoCircle size={16} />}>
                <Text size="sm">
                  Die Meldung geht an das Bundeszentralamt für Steuern und wird über „Mein ELSTER“ abgegeben (Formular „Zusammenfassende Meldung“). Dort lassen sich die Meldezeilen aus der CSV-Datei importieren.
                </Text>
              </Alert>
            </Stack>
          </Grid.Col>
        </Grid>
      )}
      <Modal opened={fileOpen} onClose={() => setFileOpen(false)} title={<Text fw={650}>Zusammenfassende Meldung übermittelt</Text>}>
        <Stack>
          <DateField label="Übermittelt am" value={fileDate} onChange={setFileDate} />
          <TextInput label="Notiz" value={note} onChange={(e) => setNote(e.currentTarget.value)} />
          <Group justify="flex-end">
            <Button variant="default" onClick={() => setFileOpen(false)}>
              Abbrechen
            </Button>
            <Button onClick={() => file.mutate()} loading={file.isPending}>
              Speichern
            </Button>
          </Group>
        </Stack>
      </Modal>
    </Page>
  );
}
