import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router';
import { Box, Grid, Group, Progress, SimpleGrid, Skeleton, Stack, Table, Text, UnstyledButton } from '@mantine/core';
import { AreaChart, BarChart, CompositeChart, DonutChart } from '@mantine/charts';
import { useApi } from '@/api/hooks';
import type { Analytics } from '@/api/types';
import { YearSelect } from '@/components/inputs';
import { KpiCard, Page, PageHeader, SectionCard } from '@/components/ui';
import { money, moneyCompact, moneyShort, num } from '@/lib/format';

const COLORS = ['brand.6', 'teal.6', 'orange.5', 'grape.5', 'cyan.6', 'pink.5', 'lime.6', 'indigo.4', 'yellow.5', 'gray.5'];

export function AnalyticsPage() {
  const navigate = useNavigate();
  const [year, setYear] = useState(new Date().getFullYear());
  const { data } = useApi<Analytics>('/reports/analytics', { year }, { keepPrevious: true });

  // In the running year, months after the current one have no data yet – leave them empty instead of drawing a flat line.
  const now = new Date();
  const lastMonth = year === now.getFullYear() ? now.getMonth() + 1 : year > now.getFullYear() ? 0 : 12;
  const hasPrev = !!data && data.months.some((m) => m.invoiced_prev !== 0);
  const cumulative = useMemo(() => {
    let r = 0;
    let p = 0;
    return (data?.months ?? []).map((m) => {
      r += m.invoiced;
      p += m.invoiced_prev;
      return { label: m.label, [String(year)]: m.month <= lastMonth ? Math.round(r * 100) / 100 : null, [String(year - 1)]: Math.round(p * 100) / 100 };
    });
  }, [data, year, lastMonth]);
  const liquidity = useMemo(() => (data?.months ?? []).map((m) => ({ label: m.label, liquidity: m.month <= lastMonth ? m.liquidity : null })), [data, lastMonth]);

  const growth = data && data.invoiced_prev_total ? ((data.invoiced_total - data.invoiced_prev_total) / Math.abs(data.invoiced_prev_total)) * 100 : null;
  const maxArticle = Math.max(1, ...(data?.top_articles ?? []).map((a) => a.amount));
  const maxCustomer = Math.max(1, ...(data?.top_customers ?? []).map((a) => a.amount));

  return (
    <Page>
      <PageHeader title="Auswertungen" subtitle="Kennzahlen, Umsatzentwicklung und Kostenstruktur" actions={<YearSelect value={year} onChange={setYear} />} />
      <SimpleGrid cols={{ base: 2, md: 3, xl: 6 }} spacing="md" mb="lg">
        <KpiCard label="Umsatz (netto)" value={data ? moneyShort(data.invoiced_total) : ''} trend={growth} hint={data?.invoiced_prev_total ? `Vorjahr ${moneyShort(data.invoiced_prev_total)}` : 'Keine Vorjahreswerte'} loading={!data} />
        <KpiCard label="Ergebnis" value={data ? moneyShort(data.profit_total) : ''} hint={data && data.revenue_total ? `Marge ${num((data.profit_total / data.revenue_total) * 100, 1)} %` : undefined} loading={!data} />
        <KpiCard label="Rechnungen" value={data?.invoice_count ?? ''} hint={`Ø ${moneyShort(data?.avg_invoice ?? 0)}`} loading={!data} />
        <KpiCard label="Ø Zahlungsdauer" value={data ? `${num(data.avg_days_to_pay, 0)} Tage` : ''} hint="bis Zahlungseingang" loading={!data} />
        <KpiCard label="Angebote" value={data?.quote_count ?? ''} hint={`Annahmequote ${num(data?.quote_conversion ?? 0, 0)} %`} loading={!data} />
        <KpiCard label="Kosten" value={data ? moneyShort(data.expense_total) : ''} hint="inkl. AfA" loading={!data} />
      </SimpleGrid>

      <Grid gap="md" mb="md">
        <Grid.Col span={{ base: 12, lg: 7 }}>
          <SectionCard title="Umsatz je Monat" description={hasPrev ? `Fakturierter Nettoumsatz ${year} im Vergleich zum Vorjahr` : `Fakturierter Nettoumsatz ${year}`}>
            {data ? (
              <BarChart
                h={300}
                data={data.months}
                dataKey="label"
                withLegend
                legendProps={{ verticalAlign: 'bottom' }}
                valueFormatter={(v) => money(v)}
                yAxisProps={{ tickFormatter: (v: number) => moneyCompact(v), width: 80 }}
                series={[
                  { name: 'invoiced', label: String(year), color: 'brand.6' },
                  ...(hasPrev ? [{ name: 'invoiced_prev', label: String(year - 1), color: 'gray.4' }] : []),
                ]}
                barProps={{ radius: 4 }}
              />
            ) : (
              <Skeleton h={300} />
            )}
          </SectionCard>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 5 }}>
          <SectionCard title="Kumulierter Umsatz" description={hasPrev ? 'Jahresverlauf im Vergleich zum Vorjahr' : 'Jahresverlauf'}>
            {data ? (
              <AreaChart
                h={300}
                data={cumulative}
                dataKey="label"
                curveType="monotone"
                withLegend
                legendProps={{ verticalAlign: 'bottom' }}
                valueFormatter={(v) => money(v)}
                yAxisProps={{ tickFormatter: (v: number) => moneyCompact(v), width: 80 }}
                series={[{ name: String(year), color: 'brand.6' }, ...(hasPrev ? [{ name: String(year - 1), color: 'gray.5' }] : [])]}
                withDots={false}
                connectNulls={false}
              />
            ) : (
              <Skeleton h={300} />
            )}
          </SectionCard>
        </Grid.Col>
      </Grid>

      <Grid gap="md" mb="md">
        <Grid.Col span={{ base: 12, lg: 7 }}>
          <SectionCard title="Erlöse, Kosten und Ergebnis" description="Laut Buchhaltung">
            {data ? (
              <CompositeChart
                h={300}
                data={data.months}
                dataKey="label"
                withLegend
                legendProps={{ verticalAlign: 'bottom' }}
                valueFormatter={(v) => money(v)}
                yAxisProps={{ tickFormatter: (v: number) => moneyCompact(v), width: 90 }}
                series={[
                  { name: 'revenue', label: 'Erlöse', color: 'teal.6', type: 'bar' },
                  { name: 'expenses', label: 'Kosten', color: 'red.4', type: 'bar' },
                  { name: 'profit', label: 'Ergebnis', color: 'brand.7', type: 'line' },
                ]}
                barProps={{ radius: 3 }}
                curveType="monotone"
              />
            ) : (
              <Skeleton h={300} />
            )}
          </SectionCard>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 5 }}>
          <SectionCard title="Liquidität" description="Bank- und Kassenbestand zum Monatsende">
            {data ? (
              <AreaChart
                h={300}
                data={liquidity}
                dataKey="label"
                curveType="monotone"
                valueFormatter={(v) => money(v)}
                yAxisProps={{ tickFormatter: (v: number) => moneyCompact(v), width: 80 }}
                series={[{ name: 'liquidity', label: 'Liquidität', color: 'cyan.6' }]}
                connectNulls={false}
                fillOpacity={0.3}
              />
            ) : (
              <Skeleton h={300} />
            )}
          </SectionCard>
        </Grid.Col>
      </Grid>

      <Grid gap="md">
        <Grid.Col span={{ base: 12, lg: 4 }}>
          <SectionCard title="Top-Kunden" description="Nach Nettoumsatz">
            {data ? (
              data.top_customers.length ? (
                <Stack gap="sm">
                  {data.top_customers.map((c) => (
                    <UnstyledButton key={c.key} onClick={() => c.id && navigate(`/kontakte/${c.id}`)}>
                      <Group justify="space-between" mb={2} wrap="nowrap">
                        <Text size="sm" truncate title={c.name}>
                          {c.name}
                        </Text>
                        <Text size="sm" fw={600} className="tabular" style={{ flexShrink: 0 }}>
                          {moneyShort(c.amount)}
                        </Text>
                      </Group>
                      <Progress value={(c.amount / maxCustomer) * 100} size="sm" color="brand" />
                    </UnstyledButton>
                  ))}
                </Stack>
              ) : (
                <Text size="sm" c="dimmed">Keine Daten</Text>
              )
            ) : (
              <Skeleton h={300} />
            )}
          </SectionCard>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 4 }}>
          <SectionCard title="Top-Produkte & Leistungen">
            <Stack gap="sm">
              {(data?.top_articles ?? []).map((a) => (
                <Box key={a.key}>
                  <Group justify="space-between" mb={2} wrap="nowrap">
                    <Text size="sm" truncate title={a.name}>
                      {a.name}
                    </Text>
                    <Text size="sm" fw={600} className="tabular" style={{ flexShrink: 0 }}>
                      {moneyShort(a.amount)}
                    </Text>
                  </Group>
                  <Progress value={(a.amount / maxArticle) * 100} size="sm" color="teal" />
                </Box>
              ))}
              {data && data.top_articles.length === 0 && <Text size="sm" c="dimmed">Keine Daten</Text>}
            </Stack>
          </SectionCard>
        </Grid.Col>
        <Grid.Col span={{ base: 12, lg: 4 }}>
          <SectionCard title="Kostenstruktur" description="Aufwand nach Konten">
            {data ? (
              <Stack align="center">
                <DonutChart
                  size={170}
                  thickness={24}
                  withTooltip
                  tooltipDataSource="segment"
                  valueFormatter={(v) => money(v)}
                  data={data.expense_categories.slice(0, 9).map((c, i) => ({ name: c.name, value: Math.round(c.amount * 100) / 100, color: COLORS[i] }))}
                />
                <Table fz="sm" verticalSpacing={4}>
                  <Table.Tbody>
                    {data.expense_categories.slice(0, 9).map((c, i) => (
                      <Table.Tr key={c.key}>
                        <Table.Td>
                          <UnstyledButton onClick={() => navigate(`/buchhaltung/konten/${c.key}?from=${year}-01-01&to=${year}-12-31`)}>
                            <Group gap={8} wrap="nowrap">
                              <Box w={10} h={10} bg={COLORS[i]} style={{ borderRadius: 3, flexShrink: 0 }} />
                              <Text size="sm" lineClamp={2} title={c.name}>
                                {c.name}
                              </Text>
                            </Group>
                          </UnstyledButton>
                        </Table.Td>
                        <Table.Td ta="right" className="tabular">
                          {moneyShort(c.amount)}
                        </Table.Td>
                      </Table.Tr>
                    ))}
                  </Table.Tbody>
                </Table>
              </Stack>
            ) : (
              <Skeleton h={300} />
            )}
          </SectionCard>
        </Grid.Col>
      </Grid>
    </Page>
  );
}
