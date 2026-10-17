// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useState } from 'react';
import { Alert, Badge, Box, Button, Card, Grid, Group, Modal, Paper, SimpleGrid, Stack, Switch, Table, Text, TextInput, ThemeIcon, UnstyledButton } from '@mantine/core';
import { IconCalculator, IconCheck, IconDownload, IconInfoCircle, IconSend, IconX } from '@tabler/icons-react';
import { api, downloadFile } from '@/api/client';
import { notifyError, useAction, useApi, useSettings } from '@/api/hooks';
import type { UstvaPeriod, UstvaReport } from '@/api/types';
import { DateField, YearSelect } from '@/components/inputs';
import { Page, PageHeader, confirmAction } from '@/components/ui';
import { date, money, num, relativeDays, today } from '@/lib/format';

function KzTag({ kz, strong }: { kz: string; strong?: boolean }) {
  return (
    <Text span size="xs" fw={700} ff="monospace" px={6} py={2} c={strong ? 'white' : 'dimmed'} bg={strong ? 'brand.6' : 'var(--app-bg)'} style={{ borderRadius: 4, border: strong ? undefined : '1px solid var(--app-border)' }}>
      {kz}
    </Text>
  );
}

function PeriodCard({ p, active, onClick }: { p: UstvaPeriod; active: boolean; onClick: () => void }) {
  const state = p.filing ? 'filed' : p.is_future ? 'future' : p.is_current ? 'current' : 'due';
  const color = { filed: 'teal', future: 'gray', current: 'blue', due: 'orange' }[state];
  return (
    <UnstyledButton onClick={onClick}>
      <Paper withBorder p="sm" radius="md" style={{ outline: active ? '2px solid var(--mantine-color-brand-5)' : undefined, opacity: p.is_future ? 0.6 : 1 }}>
        <Group justify="space-between" mb={4} wrap="nowrap">
          <Text size="sm" fw={600}>
            {p.label.replace(/ \d{4}$/, '')}
          </Text>
          <Badge size="xs" color={color} variant="light">
            {state === 'filed' ? 'gemeldet' : state === 'future' ? 'zukünftig' : state === 'current' ? 'laufend' : 'offen'}
          </Badge>
        </Group>
        <Text fw={700} className="tabular" c={p.zahllast < 0 ? 'teal.7' : undefined}>
          {money(p.zahllast)}
        </Text>
        <Text size="xs" c="dimmed">
          fällig {date(p.due_date)}
        </Text>
      </Paper>
    </UnstyledButton>
  );
}

export function UstvaPage() {
  const { data: settings } = useSettings();
  const [year, setYear] = useState(new Date().getFullYear());
  const { data: periods } = useApi<UstvaPeriod[]>('/reports/ustva', { year });
  const [period, setPeriod] = useState<string | null>(null);
  const [onlyFilled, setOnlyFilled] = useState(true);
  const [fileOpen, setFileOpen] = useState(false);
  const [fileDate, setFileDate] = useState<string | null>(today());
  const [note, setNote] = useState('Über ELSTER übermittelt');

  useEffect(() => {
    if (periods && (!period || !periods.some((p) => p.period === period))) {
      const current = periods.find((p) => p.is_current) ?? periods.filter((p) => !p.is_future).at(-1) ?? periods[0];
      setPeriod(current?.period ?? null);
    }
  }, [periods, period]);

  const { data: report } = useApi<UstvaReport>(period ? `/reports/ustva/${year}/${period}` : null);
  const file = useAction(() => api.post(`/reports/ustva/${year}/${period}/filing`, { submitted_at: fileDate, note }), { success: 'Als übermittelt markiert', onSuccess: () => setFileOpen(false) });
  const unfile = useAction(() => api.del(`/reports/ustva/${year}/${period}/filing`), { success: 'Meldung zurückgesetzt' });

  return (
    <Page>
      <PageHeader
        title="Umsatzsteuer-Voranmeldung"
        subtitle={
          settings
            ? `${settings.tax.ustva_period === 'monthly' ? 'Monatlich' : settings.tax.ustva_period === 'yearly' ? 'Jährlich' : 'Vierteljährlich'} · ${report?.basis ?? ''}${settings.tax.permanent_extension ? ' · mit Dauerfristverlängerung' : ''}`
            : undefined
        }
        actions={<YearSelect value={year} onChange={setYear} />}
      />
      {settings?.tax.small_business && (
        <Alert color="blue" icon={<IconInfoCircle size={16} />} mb="lg" title="Kleinunternehmerregelung">
          Als Kleinunternehmer nach § 19 UStG müssen Sie in der Regel keine Umsatzsteuer-Voranmeldungen abgeben. Bei Leistungen nach § 13b UStG (z. B. Software-Abos aus dem EU-Ausland) kann dennoch eine Anmeldung erforderlich sein.
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
            <Card padding={0}>
              <Group justify="space-between" p="md">
                <Box>
                  <Text fw={650} size="lg">
                    {report.period_label}
                  </Text>
                  <Text size="sm" c="dimmed">
                    {date(report.from)} – {date(report.to)}
                  </Text>
                </Box>
                <Switch label="Nur ausgefüllte Kennzahlen" checked={onlyFilled} onChange={(e) => setOnlyFilled(e.currentTarget.checked)} />
              </Group>
              <Table verticalSpacing="xs" horizontalSpacing="sm" layout="fixed">
                <Table.Thead bg="var(--app-bg)">
                  <Table.Tr>
                    <Table.Th w={56}>Kz.</Table.Th>
                    <Table.Th>Bezeichnung</Table.Th>
                    <Table.Th ta="right" w={170}>
                      Bemessungsgrundlage
                    </Table.Th>
                    <Table.Th w={56}>Kz.</Table.Th>
                    <Table.Th ta="right" w={130}>
                      Steuer
                    </Table.Th>
                  </Table.Tr>
                </Table.Thead>
                <Table.Tbody>
                  {report.sections.map((s) => {
                    const fields = s.fields.filter((f) => !onlyFilled || (f.base ?? 0) !== 0 || (f.tax ?? 0) !== 0);
                    if (fields.length === 0) return null;
                    return [
                      <Table.Tr key={s.title} bg="var(--app-bg)">
                        <Table.Td colSpan={5}>
                          <Text size="xs" fw={700} tt="uppercase" c="dimmed">
                            {s.title}
                          </Text>
                        </Table.Td>
                      </Table.Tr>,
                      ...fields.map((f) => (
                        <Table.Tr key={f.kz + f.tax_kz + f.label}>
                          <Table.Td>
                            {f.kz && <KzTag kz={f.kz} />}
                          </Table.Td>
                          <Table.Td>
                            <Text size="sm">{f.label}</Text>
                          </Table.Td>
                          <Table.Td ta="right" className="tabular">
                            {f.base !== null ? (
                              <Text size="sm" c={f.base === 0 ? 'dimmed' : undefined}>
                                {num(Math.trunc(f.base), 0)} €
                              </Text>
                            ) : null}
                          </Table.Td>
                          <Table.Td>
                            {f.tax_kz && <KzTag kz={f.tax_kz} />}
                          </Table.Td>
                          <Table.Td ta="right" className="tabular">
                            {f.tax !== null ? (
                              <Text size="sm" fw={500} c={f.tax === 0 ? 'dimmed' : undefined}>
                                {money(f.tax)}
                              </Text>
                            ) : null}
                          </Table.Td>
                        </Table.Tr>
                      )),
                    ];
                  })}
                  <Table.Tr>
                    <Table.Td>
                      <KzTag kz="83" strong />
                    </Table.Td>
                    <Table.Td colSpan={3}>
                      <Text fw={700}>{report.zahllast >= 0 ? 'Verbleibende Umsatzsteuer-Vorauszahlung' : 'Verbleibender Überschuss (Erstattung)'}</Text>
                    </Table.Td>
                    <Table.Td ta="right">
                      <Text fw={700} size="lg" className="tabular" c={report.zahllast < 0 ? 'teal.7' : undefined}>
                        {money(report.zahllast)}
                      </Text>
                    </Table.Td>
                  </Table.Tr>
                </Table.Tbody>
              </Table>
              <Text size="xs" c="dimmed" p="md">
                Bemessungsgrundlagen werden in vollen Euro angegeben (Nachkommastellen entfallen). Die Steuerbeträge entsprechen den gebuchten Werten.
              </Text>
            </Card>
          </Grid.Col>
          <Grid.Col span={{ base: 12, lg: 4 }}>
            <Stack gap="lg">
              <Card>
                <Group justify="space-between" align="flex-start">
                  <Box>
                    <Text size="sm" c="dimmed">
                      {report.zahllast >= 0 ? 'Zahllast' : 'Erstattung'}
                    </Text>
                    <Text fz={30} fw={700} className="tabular">
                      {money(Math.abs(report.zahllast))}
                    </Text>
                    <Text size="sm" c="dimmed">
                      fällig am {date(report.due_date)} ({relativeDays(report.due_date)})
                    </Text>
                  </Box>
                  <ThemeIcon size={44} radius="xl" variant="light">
                    <IconCalculator size={22} />
                  </ThemeIcon>
                </Group>
                <SimpleGrid cols={2} mt="md">
                  <Box>
                    <Text size="xs" c="dimmed">
                      Umsatzsteuer
                    </Text>
                    <Text fw={600} className="tabular">
                      {money(report.output_tax)}
                    </Text>
                  </Box>
                  <Box>
                    <Text size="xs" c="dimmed">
                      Vorsteuer
                    </Text>
                    <Text fw={600} className="tabular">
                      {money(report.input_tax)}
                    </Text>
                  </Box>
                </SimpleGrid>
                <Stack gap="xs" mt="lg">
                  {report.filing ? (
                    <Alert color="teal" icon={<IconCheck size={16} />} title="Übermittelt">
                      am {date(report.filing.submitted_at)} · {money(report.filing.amount)}
                      {report.filing.note && <Text size="xs">{report.filing.note}</Text>}
                      {Math.abs(report.filing.amount - report.zahllast) > 0.01 && (
                        <Text size="xs" c="orange.8" mt={4}>
                          Achtung: Die Werte haben sich seit der Meldung geändert ({money(report.zahllast - report.filing.amount)}). Ggf. berichtigte Voranmeldung abgeben.
                        </Text>
                      )}
                    </Alert>
                  ) : (
                    <Button leftSection={<IconSend size={16} />} onClick={() => setFileOpen(true)} disabled={new Date(report.to) > new Date()}>
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
                          message: `Die Markierung „übermittelt“ für ${report.period_label} wird entfernt. Eine bereits an das Finanzamt übermittelte Voranmeldung bleibt davon unberührt.`,
                          confirmLabel: 'Zurücksetzen',
                          onConfirm: () => unfile.mutate(),
                        })
                      }
                    >
                      Meldung zurücksetzen
                    </Button>
                  )}
                  <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile('/reports/pdf/ustva', { year, period: period ?? undefined }).catch((e) => notifyError(e))}>
                    Ausdruck (PDF)
                  </Button>
                </Stack>
              </Card>
              <Alert variant="light" color="gray" icon={<IconInfoCircle size={16} />}>
                <Text size="sm">
                  Die Übermittlung an das Finanzamt erfolgt über ELSTER (z. B. „Mein ELSTER“). Übernehmen Sie dort die Kennzahlen und buchen Sie die Zahlung anschließend unter „Bank & Zahlungen“ auf das Konto „Umsatzsteuer-Vorauszahlungen“.
                </Text>
              </Alert>
            </Stack>
          </Grid.Col>
        </Grid>
      )}
      <Modal opened={fileOpen} onClose={() => setFileOpen(false)} title={<Text fw={650}>Voranmeldung übermittelt</Text>}>
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
