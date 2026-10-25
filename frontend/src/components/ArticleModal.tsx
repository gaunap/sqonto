// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo } from 'react';
import { Autocomplete, Button, Grid, Group, Modal, SegmentedControl, Select, Stack, Switch, Text, TextInput, Textarea } from '@mantine/core';
import { useForm } from '@mantine/form';
import { api } from '@/api/client';
import { useAction, useApi } from '@/api/hooks';
import type { Article, ArticleListItem } from '@/api/types';
import { AccountSelect, DecimalInput, MoneyInput } from '@/components/inputs';
import { money } from '@/lib/format';
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

/** Dialog to create or edit a product / service; `initial` prefills a new one, `onSaved` receives the saved article. */
export function ArticleModal({
  opened,
  onClose,
  article,
  initial,
  onSaved,
}: {
  opened: boolean;
  onClose: () => void;
  article: Article | null;
  initial?: Partial<Article>;
  onSaved?: (a: Article) => void;
}) {
  const start = () => article ?? { ...emptyArticle(), ...initial };
  const form = useForm<Article>({ initialValues: start() });
  const { data: all } = useApi<ArticleListItem[]>('/articles');
  const categories = useMemo(() => [...new Set((all ?? []).map((a) => a.article.category).filter(Boolean))], [all]);
  useEffect(() => {
    if (opened) form.setValues(start());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opened, article?.id]);
  const save = useAction((a: Article) => (a.id ? api.put<Article>(`/articles/${a.id}`, a) : api.post<Article>('/articles', a)), {
    success: 'Gespeichert',
    onSuccess: (saved) => {
      onClose();
      onSaved?.(saved);
    },
  });
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
