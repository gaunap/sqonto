// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'react-router';
import { ActionIcon, Autocomplete, Badge, Button, Card, Grid, Group, Menu, Modal, SegmentedControl, Select, Stack, Switch, Text, TextInput, Textarea } from '@mantine/core';
import { useForm } from '@mantine/form';
import { IconDots, IconEdit, IconFileExport, IconPackage, IconPlus, IconSearch, IconTrash } from '@tabler/icons-react';
import { api } from '@/api/client';
import { useAction, useApi } from '@/api/hooks';
import type { Article, ArticleListItem } from '@/api/types';
import { AccountSelect, DecimalInput, MoneyInput } from '@/components/inputs';
import { ListTable } from '@/components/ListTable';
import { Empty, Page, PageHeader, confirmAction } from '@/components/ui';
import { exportCsv } from '@/lib/csv';
import { money, qty } from '@/lib/format';
import { TAX_RATES, UNITS } from '@/lib/labels';

function emptyArticle(): Article {
  return {
    id: 0,
    number: '',
    kind: 'service',
    name: '',
    description: '',
    unit: 'Std',
    price: 0,
    purchase_price: 0,
    tax_rate: 19,
    revenue_account: '',
    category: '',
    gtin: '',
    track_stock: false,
    stock: 0,
    archived: false,
    created_at: '',
    updated_at: '',
  };
}

export function ArticleModal({ opened, onClose, article }: { opened: boolean; onClose: () => void; article: Article | null }) {
  const form = useForm<Article>({ initialValues: article ?? emptyArticle() });
  const { data: all } = useApi<ArticleListItem[]>('/articles');
  const categories = useMemo(() => [...new Set((all ?? []).map((a) => a.article.category).filter(Boolean))], [all]);
  useEffect(() => {
    if (opened) form.setValues(article ?? emptyArticle());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opened, article?.id]);
  const save = useAction((a: Article) => (a.id ? api.put(`/articles/${a.id}`, a) : api.post('/articles', a)), { success: 'Gespeichert', onSuccess: onClose });
  const v = form.values;
  const gross = Math.round(v.price * (1 + v.tax_rate / 100) * 100) / 100;
  const margin = v.purchase_price > 0 && v.price > 0 ? ((v.price - v.purchase_price) / v.price) * 100 : null;
  return (
    <Modal opened={opened} onClose={onClose} size="lg" title={<Text fw={650}>{v.id ? 'Produkt / Leistung bearbeiten' : 'Neues Produkt / neue Leistung'}</Text>}>
      <form onSubmit={form.onSubmit((values) => save.mutate(values))}>
        <Stack gap="sm">
          <SegmentedControl
            value={v.kind}
            onChange={(k) => {
              form.setFieldValue('kind', k);
              if (!v.id) form.setFieldValue('unit', k === 'service' ? 'Std' : 'Stk');
            }}
            data={[
              { value: 'service', label: 'Dienstleistung' },
              { value: 'product', label: 'Produkt' },
            ]}
          />
          <Grid gap="sm">
            <Grid.Col span={{ base: 12, sm: 8 }}>
              <TextInput label="Bezeichnung" required data-autofocus {...form.getInputProps('name')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 4 }}>
              <TextInput label="Artikelnummer" placeholder="automatisch" {...form.getInputProps('number')} />
            </Grid.Col>
            <Grid.Col span={12}>
              <Textarea label="Beschreibung" description="Wird als zweite Zeile in Dokumenten übernommen" minRows={2} {...form.getInputProps('description')} />
            </Grid.Col>
            <Grid.Col span={{ base: 6, sm: 4 }}>
              <MoneyInput label="Verkaufspreis netto" value={v.price} onChange={(x) => form.setFieldValue('price', x)} decimalScale={4} fixedDecimalScale={false} />
            </Grid.Col>
            <Grid.Col span={{ base: 6, sm: 4 }}>
              <Select label="Umsatzsteuer" data={TAX_RATES} value={String(v.tax_rate)} onChange={(x) => form.setFieldValue('tax_rate', Number(x ?? 19))} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 4 }}>
              <Autocomplete label="Einheit" data={UNITS} {...form.getInputProps('unit')} />
            </Grid.Col>
            <Grid.Col span={12}>
              <Text size="xs" c="dimmed">
                Bruttopreis: {money(gross)}
                {margin !== null && ` · Marge ${margin.toFixed(1)} %`}
              </Text>
            </Grid.Col>
            <Grid.Col span={{ base: 6, sm: 4 }}>
              <MoneyInput label="Einkaufspreis netto" value={v.purchase_price} onChange={(x) => form.setFieldValue('purchase_price', x)} />
            </Grid.Col>
            <Grid.Col span={{ base: 6, sm: 4 }}>
              <Autocomplete label="Kategorie" data={categories} {...form.getInputProps('category')} />
            </Grid.Col>
            <Grid.Col span={{ base: 12, sm: 4 }}>
              <TextInput label="GTIN / EAN" {...form.getInputProps('gtin')} />
            </Grid.Col>
            <Grid.Col span={12}>
              <AccountSelect label="Erlöskonto" description="Leer = automatisch nach Steuersatz" picks={['revenue']} value={v.revenue_account} onChange={(x) => form.setFieldValue('revenue_account', x ?? '')} clearable allowDeselect />
            </Grid.Col>
            {v.kind === 'product' && (
              <>
                <Grid.Col span={{ base: 6 }}>
                  <Switch mt="md" label="Lagerbestand führen" checked={v.track_stock} onChange={(e) => form.setFieldValue('track_stock', e.currentTarget.checked)} />
                </Grid.Col>
                {v.track_stock && (
                  <Grid.Col span={{ base: 6 }}>
                    <DecimalInput label="Bestand" value={v.stock} onChange={(x) => form.setFieldValue('stock', x)} />
                  </Grid.Col>
                )}
              </>
            )}
          </Grid>
          <Group justify="flex-end" mt="sm">
            <Button variant="default" onClick={onClose}>
              Abbrechen
            </Button>
            <Button type="submit" loading={save.isPending}>
              Speichern
            </Button>
          </Group>
        </Stack>
      </form>
    </Modal>
  );
}

export function ArticlesPage() {
  const [params, setParams] = useSearchParams();
  const [q, setQ] = useState('');
  const [kind, setKind] = useState('all');
  const [editing, setEditing] = useState<Article | null>(null);
  const [open, setOpen] = useState(false);
  const { data, isFetching } = useApi<ArticleListItem[]>('/articles');
  const remove = useAction((id: number) => api.del<{ message: string }>(`/articles/${id}`), { success: (r) => (r.message === 'archived' ? 'Archiviert (wird in Dokumenten verwendet)' : 'Gelöscht') });

  useEffect(() => {
    const e = params.get('edit');
    if (params.get('neu') === '1') {
      setEditing(null);
      setOpen(true);
      setParams({}, { replace: true });
    } else if (e && data) {
      const a = data.find((x) => x.article.id === Number(e));
      if (a) {
        setEditing(a.article);
        setOpen(true);
        setParams({}, { replace: true });
      }
    }
  }, [params, data, setParams]);

  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    return (data ?? []).filter((a) => (kind === 'all' || a.article.kind === kind) && (!s || `${a.article.name} ${a.article.number} ${a.article.category}`.toLowerCase().includes(s)));
  }, [data, q, kind]);

  return (
    <Page>
      <PageHeader
        title="Produkte & Leistungen"
        subtitle="Artikelkatalog für Angebote und Rechnungen"
        actions={
          <>
            <Button
              variant="default"
              leftSection={<IconFileExport size={16} />}
              onClick={() =>
                exportCsv('Artikel', filtered, [
                  { label: 'Nummer', value: (a) => a.article.number },
                  { label: 'Bezeichnung', value: (a) => a.article.name },
                  { label: 'Einheit', value: (a) => a.article.unit },
                  { label: 'Preis netto', value: (a) => a.article.price },
                  { label: 'USt %', value: (a) => a.article.tax_rate },
                  { label: 'Kategorie', value: (a) => a.article.category },
                  { label: 'Umsatz', value: (a) => a.revenue },
                ])
              }
            >
              Export
            </Button>
            <Button
              leftSection={<IconPlus size={16} />}
              onClick={() => {
                setEditing(null);
                setOpen(true);
              }}
            >
              Neu
            </Button>
          </>
        }
      />
      <Stack gap="md">
        <Group justify="space-between">
          <SegmentedControl
            value={kind}
            onChange={setKind}
            data={[
              { value: 'all', label: 'Alle' },
              { value: 'service', label: 'Dienstleistungen' },
              { value: 'product', label: 'Produkte' },
            ]}
          />
          <TextInput placeholder="Suchen …" leftSection={<IconSearch size={16} />} value={q} onChange={(e) => setQ(e.currentTarget.value)} w={260} />
        </Group>
        <Card padding={0}>
          {data && data.length === 0 ? (
            <Empty
              icon={<IconPackage size={22} />}
              title="Noch keine Produkte oder Leistungen"
              description="Legen Sie wiederkehrende Leistungen an, um Rechnungen schneller zu schreiben."
              action={
                <Button leftSection={<IconPlus size={16} />} onClick={() => setOpen(true)}>
                  Anlegen
                </Button>
              }
            />
          ) : (
            <ListTable<ArticleListItem>
              records={filtered}
              idAccessor={(a) => a.article.id}
              fetching={isFetching}
              defaultSort={{ column: 'article.name', direction: 'asc' }}
              onRowClick={(a) => {
                setEditing(a.article);
                setOpen(true);
              }}
              columns={[
                {
                  accessor: 'article.name',
                  title: 'Bezeichnung',
                  sortable: true,
                  render: (a) => (
                    <Stack gap={0}>
                      <Text size="sm" fw={600}>
                        {a.article.name}
                      </Text>
                      <Text size="xs" c="dimmed" lineClamp={1}>
                        {a.article.description}
                      </Text>
                    </Stack>
                  ),
                },
                { accessor: 'article.number', title: 'Nummer', sortable: true, width: 110 },
                { accessor: 'article.kind', title: 'Art', width: 140, render: (a) => <Badge variant="light" color={a.article.kind === 'service' ? 'brand' : 'teal'}>{a.article.kind === 'service' ? 'Dienstleistung' : 'Produkt'}</Badge> },
                { accessor: 'article.category', title: 'Kategorie', sortable: true, visibleMediaQuery: '(min-width: 62em)' },
                { accessor: 'article.price', title: 'Preis netto', sortable: true, textAlign: 'right', render: (a) => <Text size="sm" className="tabular" fw={500}>{money(a.article.price)}<Text span size="xs" c="dimmed"> / {a.article.unit}</Text></Text> },
                { accessor: 'article.tax_rate', title: 'USt', width: 70, textAlign: 'right', render: (a) => `${a.article.tax_rate} %` },
                { accessor: 'revenue', title: 'Umsatz', sortable: true, textAlign: 'right', visibleMediaQuery: '(min-width: 48em)', render: (a) => <Text size="sm" className="tabular" c="dimmed">{a.revenue ? `${money(a.revenue)} (${qty(a.sold_quantity)})` : '–'}</Text> },
                {
                  accessor: 'actions',
                  title: '',
                  width: 50,
                  render: (a) => (
                    <Menu position="bottom-end">
                      <Menu.Target>
                        <ActionIcon onClick={(e) => e.stopPropagation()} aria-label="Aktionen">
                          <IconDots size={16} />
                        </ActionIcon>
                      </Menu.Target>
                      <Menu.Dropdown onClick={(e) => e.stopPropagation()}>
                        <Menu.Item leftSection={<IconEdit size={14} />} onClick={() => { setEditing(a.article); setOpen(true); }}>
                          Bearbeiten
                        </Menu.Item>
                        <Menu.Item color="red" leftSection={<IconTrash size={14} />} onClick={() => confirmAction({ title: 'Löschen', message: `„${a.article.name}“ löschen?`, danger: true, confirmLabel: 'Löschen', onConfirm: () => remove.mutate(a.article.id) })}>
                          Löschen
                        </Menu.Item>
                      </Menu.Dropdown>
                    </Menu>
                  ),
                },
              ]}
            />
          )}
        </Card>
      </Stack>
      <ArticleModal opened={open} onClose={() => setOpen(false)} article={editing} />
    </Page>
  );
}
