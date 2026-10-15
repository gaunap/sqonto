import { Fragment, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router';
import { Alert, Anchor, Badge, Box, Button, Card, Group, ScrollArea, SimpleGrid, Skeleton, Stack, Table, Tabs, Text } from '@mantine/core';
import { IconChevronDown, IconChevronRight, IconDownload, IconInfoCircle } from '@tabler/icons-react';
import { downloadFile } from '@/api/client';
import { notifyError, useApi, useStatus } from '@/api/hooks';
import type { Bwa, Statement, StatementSection, SusaRow } from '@/api/types';
import { YearSelect } from '@/components/inputs';
import { Page, PageHeader } from '@/components/ui';
import { exportCsv } from '@/lib/csv';
import { money, num } from '@/lib/format';
import { ACCOUNT_KINDS } from '@/lib/labels';

function change(a: number, b: number) {
  if (!b) return null;
  return ((a - b) / Math.abs(b)) * 100;
}

function SectionTable({ section, year, showPrev = true, prevLabel }: { section: StatementSection; year: number; showPrev?: boolean; prevLabel: string }) {
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const navigate = useNavigate();
  const visible = section.rows.filter((r) => r.amount !== 0 || r.prev !== 0 || r.style !== 'normal');
  return (
    <Table verticalSpacing={8} horizontalSpacing="md" highlightOnHover>
      {section.title && (
        <Table.Thead bg="var(--app-bg)">
          <Table.Tr>
            <Table.Th>{section.title}</Table.Th>
            <Table.Th ta="right" w={150}>
              {year}
            </Table.Th>
            {showPrev && (
              <Table.Th ta="right" w={150}>
                {prevLabel}
              </Table.Th>
            )}
            {showPrev && (
              <Table.Th ta="right" w={90}>
                Δ
              </Table.Th>
            )}
          </Table.Tr>
        </Table.Thead>
      )}
      <Table.Tbody>
        {visible.length === 0 && (
          <Table.Tr>
            <Table.Td colSpan={4}>
              <Text size="sm" c="dimmed">
                Keine Buchungen
              </Text>
            </Table.Td>
          </Table.Tr>
        )}
        {visible.map((r, i) => {
          const key = r.key || `r${i}`;
          const expandable = r.accounts.length > 0;
          const ch = change(r.amount, r.prev);
          const isSub = r.style === 'subtotal' || r.style === 'total';
          return (
            <Fragment key={key}>
              <Table.Tr style={{ cursor: expandable ? 'pointer' : undefined, background: isSub ? 'var(--app-bg)' : undefined }} onClick={() => expandable && setOpen((o) => ({ ...o, [key]: !o[key] }))}>
                <Table.Td>
                  <Group gap={6} wrap="nowrap">
                    {expandable ? open[key] ? <IconChevronDown size={14} /> : <IconChevronRight size={14} /> : <Box w={14} />}
                    <Text size="sm" fw={isSub ? 700 : 500} c={r.style === 'info' ? 'dimmed' : undefined}>
                      {r.label}
                    </Text>
                  </Group>
                </Table.Td>
                <Table.Td ta="right">
                  <Text size="sm" fw={isSub ? 700 : 600} className="tabular" c={r.amount < 0 && isSub ? 'red.7' : undefined}>
                    {money(r.amount)}
                  </Text>
                </Table.Td>
                {showPrev && (
                  <Table.Td ta="right">
                    <Text size="sm" c="dimmed" className="tabular">
                      {money(r.prev)}
                    </Text>
                  </Table.Td>
                )}
                {showPrev && (
                  <Table.Td ta="right">
                    {ch !== null && Number.isFinite(ch) && (
                      <Text size="xs" c={ch >= 0 ? 'teal.7' : 'red.7'}>
                        {ch >= 0 ? '+' : ''}
                        {ch.toFixed(0)} %
                      </Text>
                    )}
                  </Table.Td>
                )}
              </Table.Tr>
              {expandable &&
                open[key] &&
                r.accounts.map((a) => (
                  <Table.Tr key={key + a.number + a.name}>
                    <Table.Td pl={44}>
                      {a.number && /^\d+$/.test(a.number) ? (
                        <Anchor size="sm" onClick={() => navigate(`/buchhaltung/konten/${a.number}?from=${year}-01-01&to=${year}-12-31`)}>
                          {a.number} · {a.name}
                        </Anchor>
                      ) : (
                        <Text size="sm" c="dimmed">
                          {a.name}
                        </Text>
                      )}
                    </Table.Td>
                    <Table.Td ta="right">
                      <Text size="sm" className="tabular">
                        {money(a.amount)}
                      </Text>
                    </Table.Td>
                    {showPrev && (
                      <Table.Td ta="right">
                        <Text size="sm" c="dimmed" className="tabular">
                          {money(a.prev)}
                        </Text>
                      </Table.Td>
                    )}
                    {showPrev && <Table.Td />}
                  </Table.Tr>
                ))}
            </Fragment>
          );
        })}
        {section.total_label && (
          <Table.Tr bg="var(--app-bg)">
            <Table.Td>
              <Text fw={700} pl={20}>
                {section.total_label}
              </Text>
            </Table.Td>
            <Table.Td ta="right">
              <Text fw={700} className="tabular">
                {money(section.total)}
              </Text>
            </Table.Td>
            {showPrev && (
              <Table.Td ta="right">
                <Text fw={600} c="dimmed" className="tabular">
                  {money(section.prev_total)}
                </Text>
              </Table.Td>
            )}
            {showPrev && <Table.Td />}
          </Table.Tr>
        )}
      </Table.Tbody>
    </Table>
  );
}

function StatementView({ path, year, pdf, params }: { path: string; year: number; pdf: string; params?: Record<string, string | number> }) {
  const { data } = useApi<Statement>(path, { year, ...params });
  if (!data) return <Skeleton h={400} />;
  const isBilanz = path.includes('bilanz');
  // Prior-year columns are only useful when there is prior-year data.
  const hasPrev = data.prev_result !== 0 || data.sections.some((s) => s.prev_total !== 0 || s.rows.some((r) => r.prev !== 0));
  return (
    <Stack gap="lg">
      {!data.applicable && (
        <Alert color="yellow" icon={<IconInfoCircle size={16} />}>
          {isBilanz ? 'Sie nutzen die Einnahmen-Überschuss-Rechnung. Die Bilanz dient hier nur zur Information.' : 'Sie bilanzieren. Für die Gewinnermittlung ist die GuV maßgeblich.'}
        </Alert>
      )}
      <Group justify="space-between" align="flex-end">
        <Box>
          <Text fw={700} size="xl">
            {data.title}
          </Text>
          <Text size="sm" c="dimmed">
            {data.subtitle}
          </Text>
        </Box>
        <Group>
          <Card padding="sm" withBorder>
            <Text size="xs" c="dimmed">
              {data.result_label}
            </Text>
            <Text fz={24} fw={700} className="tabular" c={data.result < 0 ? 'red.7' : 'teal.7'}>
              {money(data.result)}
            </Text>
          </Card>
          <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile(`/reports/pdf/${pdf}`, { year, ...params }).catch((e) => notifyError(e))}>
            PDF
          </Button>
        </Group>
      </Group>
      {isBilanz ? (
        <SimpleGrid cols={{ base: 1, lg: 2 }}>
          {data.sections.map((s) => (
            <Card key={s.title} padding={0}>
              <SectionTable section={s} year={year} prevLabel="Vorjahr" showPrev={hasPrev} />
            </Card>
          ))}
        </SimpleGrid>
      ) : (
        <Card padding={0}>
          {data.sections.map((s, i) => (
            <SectionTable key={i} section={s} year={year} prevLabel={`${year - 1}`} showPrev={hasPrev} />
          ))}
          <Group justify="space-between" p="md" bg="var(--mantine-color-brand-0)">
            <Text fw={700}>{data.result_label}</Text>
            <Group gap="xl">
              <Text fw={700} size="lg" className="tabular">
                {money(data.result)}
              </Text>
              {hasPrev && (
                <Text c="dimmed" className="tabular" w={150} ta="right">
                  {money(data.prev_result)}
                </Text>
              )}
            </Group>
          </Group>
        </Card>
      )}
      {data.notes.map((n) => (
        <Text key={n} size="xs" c="dimmed">
          {n}
        </Text>
      ))}
    </Stack>
  );
}

const stickyCol = (bg: string) => ({ position: 'sticky' as const, left: 0, zIndex: 1, background: bg, boxShadow: 'inset -1px 0 0 var(--app-border)' });

function BwaView({ year }: { year: number }) {
  const { data } = useApi<Bwa>('/reports/bwa', { year });
  if (!data) return <Skeleton h={400} />;
  return (
    <Stack>
      <Group justify="space-between">
        <Box>
          <Text fw={700} size="xl">
            Betriebswirtschaftliche Auswertung
          </Text>
          <Text size="sm" c="dimmed">
            Kurzfristige Erfolgsrechnung {year} nach Monaten
          </Text>
        </Box>
        <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile('/reports/pdf/bwa', { year }).catch((e) => notifyError(e))}>
          PDF
        </Button>
      </Group>
      <Card padding={0}>
        <ScrollArea>
          <Table verticalSpacing={6} horizontalSpacing="sm" miw={1200} fz="sm">
            <Table.Thead bg="var(--app-bg)">
              <Table.Tr>
                <Table.Th miw={240} style={stickyCol('var(--app-bg)')}>
                  Position
                </Table.Th>
                {data.months.map((m) => (
                  <Table.Th key={m} ta="right">
                    {m}
                  </Table.Th>
                ))}
                <Table.Th ta="right">Gesamt</Table.Th>
                <Table.Th ta="right">% Umsatz</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {data.rows.map((r) => {
                const strong = r.style !== 'normal';
                return (
                  <Table.Tr key={r.key} bg={strong ? 'var(--app-bg)' : undefined}>
                    <Table.Td style={stickyCol(strong ? 'var(--app-bg)' : 'var(--app-surface)')}>
                      <Text size="sm" fw={strong ? 700 : 400}>
                        {r.label}
                      </Text>
                    </Table.Td>
                    {r.values.map((v, i) => (
                      <Table.Td key={i} ta="right" className="tabular">
                        <Text size="sm" fw={strong ? 600 : 400} c={v === 0 ? 'dimmed' : v < 0 && strong ? 'red.7' : undefined}>
                          {num(v, 0)}
                        </Text>
                      </Table.Td>
                    ))}
                    <Table.Td ta="right" className="tabular">
                      <Text size="sm" fw={700} c={r.total < 0 && strong ? 'red.7' : undefined}>
                        {num(r.total, 0)}
                      </Text>
                    </Table.Td>
                    <Table.Td ta="right">
                      <Text size="xs" c="dimmed">
                        {r.share ? `${num(r.share, 1)} %` : ''}
                      </Text>
                    </Table.Td>
                  </Table.Tr>
                );
              })}
            </Table.Tbody>
          </Table>
        </ScrollArea>
      </Card>
      <Text size="xs" c="dimmed">
        Beträge in Euro. Die BWA basiert auf den gebuchten Erlösen und Aufwendungen.
      </Text>
    </Stack>
  );
}

/** Balance as absolute amount with Soll/Haben marker. */
const sh = (v: number) => (Math.abs(v) < 0.005 ? money(0) : `${money(Math.abs(v))} ${v > 0 ? 'S' : 'H'}`);

function SusaView({ year }: { year: number }) {
  const { data } = useApi<SusaRow[]>('/reports/susa', { year });
  if (!data) return <Skeleton h={400} />;
  const sd = data.reduce((s, r) => s + r.debit, 0);
  const sc = data.reduce((s, r) => s + r.credit, 0);
  return (
    <Stack>
      <Group justify="space-between">
        <Box>
          <Text fw={700} size="xl">
            Summen- und Saldenliste
          </Text>
          <Text size="sm" c="dimmed">
            01.01.{year} – 31.12.{year}
          </Text>
        </Box>
        <Group>
          <Button
            variant="default"
            onClick={() =>
              exportCsv(`SuSa_${year}`, data, [
                { label: 'Konto', value: (r) => r.number },
                { label: 'Bezeichnung', value: (r) => r.name },
                { label: 'Saldovortrag', value: (r) => r.opening },
                { label: 'Soll', value: (r) => r.debit },
                { label: 'Haben', value: (r) => r.credit },
                { label: 'Saldo', value: (r) => r.closing },
              ])
            }
          >
            CSV
          </Button>
          <Button variant="default" leftSection={<IconDownload size={16} />} onClick={() => downloadFile('/reports/pdf/susa', { year }).catch((e) => notifyError(e))}>
            PDF
          </Button>
        </Group>
      </Group>
      <Card padding={0}>
        <Table.ScrollContainer minWidth={860}>
          <Table verticalSpacing={6} highlightOnHover>
            <Table.Thead bg="var(--app-bg)">
              <Table.Tr>
                <Table.Th>Konto</Table.Th>
                <Table.Th>Bezeichnung</Table.Th>
                <Table.Th>Art</Table.Th>
                <Table.Th ta="right">Saldovortrag</Table.Th>
                <Table.Th ta="right">Soll</Table.Th>
                <Table.Th ta="right">Haben</Table.Th>
                <Table.Th ta="right">Saldo</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {data.map((r) => (
                <Table.Tr key={r.number}>
                  <Table.Td>
                    <Anchor component={Link} to={`/buchhaltung/konten/${r.number}?from=${year}-01-01&to=${year}-12-31`} size="sm" fw={600}>
                      {r.number}
                    </Anchor>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm">{r.name}</Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="xs" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
                      {ACCOUNT_KINDS[r.kind] ?? r.kind}
                    </Text>
                  </Table.Td>
                  <Table.Td ta="right" className="tabular">
                    <Text size="sm" c="dimmed" style={{ whiteSpace: 'nowrap' }}>
                      {sh(r.opening)}
                    </Text>
                  </Table.Td>
                  <Table.Td ta="right" className="tabular">
                    <Text size="sm">{money(r.debit)}</Text>
                  </Table.Td>
                  <Table.Td ta="right" className="tabular">
                    <Text size="sm">{money(r.credit)}</Text>
                  </Table.Td>
                  <Table.Td ta="right" className="tabular">
                    <Text size="sm" fw={600} style={{ whiteSpace: 'nowrap' }}>
                      {sh(r.closing)}
                    </Text>
                  </Table.Td>
                </Table.Tr>
              ))}
              <Table.Tr bg="var(--app-bg)">
                <Table.Td colSpan={4}>
                  <Text fw={700}>Summe</Text>
                </Table.Td>
                <Table.Td ta="right" className="tabular">
                  <Text fw={700}>{money(sd)}</Text>
                </Table.Td>
                <Table.Td ta="right" className="tabular">
                  <Text fw={700}>{money(sc)}</Text>
                </Table.Td>
                <Table.Td ta="right">
                  {Math.abs(sd - sc) < 0.01 ? (
                    <Badge color="teal" variant="light">
                      ausgeglichen
                    </Badge>
                  ) : (
                    <Badge color="red">Differenz {money(sd - sc)}</Badge>
                  )}
                </Table.Td>
              </Table.Tr>
            </Table.Tbody>
          </Table>
        </Table.ScrollContainer>
      </Card>
    </Stack>
  );
}

export function StatementsPage() {
  const { data: status } = useStatus();
  const [params, setParams] = useSearchParams();
  const [year, setYear] = useState(new Date().getFullYear());
  const defaultTab = status?.bookkeeping === 'bilanz' ? 'guv' : 'euer';
  const tab = params.get('tab') ?? defaultTab;
  return (
    <Page>
      <PageHeader title="Jahresabschluss & Berichte" subtitle="Gewinnermittlung, Bilanz und betriebswirtschaftliche Auswertungen" actions={<YearSelect value={year} onChange={setYear} />} />
      <Tabs value={tab} onChange={(v) => setParams({ tab: v ?? defaultTab })}>
        <Tabs.List mb="lg">
          <Tabs.Tab value="euer">EÜR</Tabs.Tab>
          <Tabs.Tab value="guv">GuV</Tabs.Tab>
          <Tabs.Tab value="bilanz">Bilanz</Tabs.Tab>
          <Tabs.Tab value="bwa">BWA</Tabs.Tab>
          <Tabs.Tab value="susa">Summen- & Saldenliste</Tabs.Tab>
        </Tabs.List>
        <Tabs.Panel value="euer">
          <StatementView path="/reports/euer" year={year} pdf="euer" />
        </Tabs.Panel>
        <Tabs.Panel value="guv">
          <StatementView path="/reports/guv" year={year} pdf="guv" />
        </Tabs.Panel>
        <Tabs.Panel value="bilanz">
          <StatementView path="/reports/bilanz" year={year} pdf="bilanz" />
        </Tabs.Panel>
        <Tabs.Panel value="bwa">
          <BwaView year={year} />
        </Tabs.Panel>
        <Tabs.Panel value="susa">
          <SusaView year={year} />
        </Tabs.Panel>
      </Tabs>
    </Page>
  );
}
