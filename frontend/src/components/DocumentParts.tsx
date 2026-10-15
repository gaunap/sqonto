import { Box, Center, Divider, Group, Loader, Skeleton, Stack, Text } from '@mantine/core';
import type { Document, Totals } from '@/api/types';
import { money, percent } from '@/lib/format';

export function TotalsBox({ totals, doc }: { totals: Totals; doc: Pick<Document, 'tax_mode' | 'prices_gross' | 'kind'> }) {
  const standard = doc.tax_mode === 'standard';
  const row = (label: React.ReactNode, value: number, opts: { bold?: boolean; muted?: boolean; color?: string } = {}) => (
    <Group justify="space-between" gap="xl" wrap="nowrap">
      <Text size={opts.bold ? 'md' : 'sm'} fw={opts.bold ? 700 : 400} c={opts.muted ? 'dimmed' : undefined}>
        {label}
      </Text>
      <Text size={opts.bold ? 'md' : 'sm'} fw={opts.bold ? 700 : 500} className="tabular" c={opts.color ?? (opts.muted ? 'dimmed' : undefined)}>
        {money(value)}
      </Text>
    </Group>
  );
  const hasDeductions = totals.deductions.length > 0;
  return (
    <Stack gap={6} maw={420} ml="auto" w="100%">
      {hasDeductions ? (
        <>
          {row('Gesamtleistung netto', totals.items_net)}
          {standard && totals.item_groups.filter((g) => g.rate > 0).map((g) => <Box key={g.rate}>{row(`zzgl. ${percent(g.rate)} USt`, g.tax)}</Box>)}
          {row('Gesamtleistung brutto', totals.items_gross, { bold: false })}
          {totals.deductions.map((d, i) => (
            <Box key={i}>{row(d.label, d.gross, { color: 'orange.8' })}</Box>
          ))}
          <Divider my={4} />
          {row(totals.gross >= 0 ? 'Restbetrag' : 'Guthaben', totals.gross, { bold: true })}
        </>
      ) : doc.prices_gross && standard ? (
        <>
          {row('Gesamtbetrag (brutto)', totals.gross, { bold: true })}
          {totals.groups.filter((g) => g.rate > 0).map((g) => <Box key={g.rate}>{row(`darin ${percent(g.rate)} USt auf ${money(g.net)}`, g.tax, { muted: true })}</Box>)}
          {row('Nettobetrag', totals.net, { muted: true })}
        </>
      ) : (
        <>
          {standard && row('Nettobetrag', totals.net)}
          {standard &&
            totals.groups
              .filter((g) => g.rate > 0)
              .map((g) => <Box key={g.rate}>{row(`zzgl. ${percent(g.rate)} USt auf ${money(g.net)}`, g.tax)}</Box>)}
          <Divider my={4} />
          {row('Gesamtbetrag', totals.gross, { bold: true })}
        </>
      )}
    </Stack>
  );
}

/** Renders typst SVG pages. */
export function PagesPreview({ pages, loading, error, maxWidth = 760 }: { pages: string[] | undefined; loading?: boolean; error?: string | null; maxWidth?: number }) {
  if (error) {
    return (
      <Center p="xl">
        <Text c="red" size="sm" style={{ whiteSpace: 'pre-wrap' }}>
          {error}
        </Text>
      </Center>
    );
  }
  if (!pages) {
    return (
      <Box className="pdf-page" style={{ aspectRatio: '210 / 297', maxWidth }} mx="auto">
        <Skeleton h="100%" />
      </Box>
    );
  }
  return (
    <Box pos="relative" maw={maxWidth} mx="auto">
      {loading && (
        <Center pos="absolute" top={8} right={8} style={{ zIndex: 2 }}>
          <Loader size="xs" />
        </Center>
      )}
      <div className="pdf-pages">
        {pages.map((svg, i) => (
          <div key={i} className="pdf-page" dangerouslySetInnerHTML={{ __html: svg }} />
        ))}
      </div>
    </Box>
  );
}
