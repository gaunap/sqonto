// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useMemo, useState } from 'react';
import {
  ActionIcon,
  Autocomplete,
  Badge,
  Box,
  Button,
  Combobox,
  Group,
  Menu,
  Select,
  Stack,
  Table,
  Text,
  TextInput,
  Textarea,
  Tooltip,
  useCombobox,
} from '@mantine/core';
import {
  IconCopy,
  IconDotsVertical,
  IconGripVertical,
  IconLetterT,
  IconPackage,
  IconPlus,
  IconSum,
  IconTrash,
  IconTextPlus,
} from '@tabler/icons-react';
import { DndContext, closestCenter, PointerSensor, KeyboardSensor, useSensor, useSensors, type DragEndEvent } from '@dnd-kit/core';
import { SortableContext, useSortable, verticalListSortingStrategy, arrayMove, sortableKeyboardCoordinates } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { useArticles } from '@/api/hooks';
import type { Article, Document, DocumentLine } from '@/api/types';
import { calculate } from '@/lib/calc';
import { money, qty as fmtQty } from '@/lib/format';
import { TAX_RATES, UNITS } from '@/lib/labels';
import { ArticleModal } from './ArticleModal';
import { DecimalInput, MoneyInput } from './inputs';

export type EditorLine = DocumentLine & { _key: string };

let keySeq = 0;
export const newKey = () => `l${Date.now().toString(36)}${(keySeq++).toString(36)}`;

/**
 * Unit price with two decimals like every amount; prices with sub-cent precision (e.g. from
 * the article catalog, 0,0125 € per unit) keep up to four decimals.
 */
function PriceInput({ value, onChange, readOnly }: { value: number; onChange: (v: number) => void; readOnly?: boolean }) {
  const wholeCents = Math.abs(value * 100 - Math.round(value * 100)) < 1e-6;
  return <MoneyInput value={value} onChange={onChange} decimalScale={wholeCents ? 2 : 4} fixedDecimalScale={wholeCents} readOnly={readOnly} aria-label="Einzelpreis" />;
}

export function emptyLine(defaults: Partial<DocumentLine> = {}): EditorLine {
  return {
    _key: newKey(),
    id: 0,
    document_id: 0,
    position: 0,
    line_type: 'item',
    article_id: null,
    article_number: '',
    name: '',
    description: '',
    quantity: 1,
    unit: 'Stk',
    unit_price: 0,
    discount_percent: 0,
    tax_rate: 19,
    account: '',
    net_amount: 0,
    tax_amount: 0,
    gross_amount: 0,
    ref_document_id: null,
    ...defaults,
  };
}

export function lineFromArticle(a: Article, base?: Partial<EditorLine>): Partial<EditorLine> {
  return {
    ...base,
    article_id: a.id,
    article_number: a.number,
    name: a.name,
    description: a.description,
    unit: a.unit,
    unit_price: a.price,
    tax_rate: a.tax_rate,
    account: a.revenue_account,
  };
}

const CREATE_ARTICLE = 'create-article';

function ArticleNameInput({
  value,
  onChange,
  onPick,
  onCreate,
  articles,
  readOnly,
}: {
  value: string;
  onChange: (v: string) => void;
  onPick: (a: Article) => void;
  onCreate: (name: string) => void;
  articles: Article[];
  readOnly?: boolean;
}) {
  const combobox = useCombobox({ onDropdownClose: () => combobox.resetSelectedOption() });
  const q = value.trim().toLowerCase();
  const filtered = useMemo(
    () => (q.length === 0 ? articles.slice(0, 8) : articles.filter((a) => `${a.name} ${a.number} ${a.category}`.toLowerCase().includes(q)).slice(0, 8)),
    [q, articles],
  );
  // Offered for a name the catalog does not have yet.
  const canCreate = q.length > 0 && !articles.some((a) => a.name.trim().toLowerCase() === q);
  return (
    <Combobox
      store={combobox}
      onOptionSubmit={(id) => {
        if (id === CREATE_ARTICLE) {
          onCreate(value.trim());
          combobox.closeDropdown();
          return;
        }
        const a = articles.find((x) => String(x.id) === id);
        if (a) onPick(a);
        combobox.closeDropdown();
      }}
      withinPortal
      position="bottom-start"
      width={420}
    >
      <Combobox.Target>
        <TextInput
          value={value}
          readOnly={readOnly}
          placeholder="Bezeichnung oder Artikel suchen …"
          onChange={(e) => {
            onChange(e.currentTarget.value);
            combobox.openDropdown();
            combobox.updateSelectedOptionIndex();
          }}
          onFocus={() => !readOnly && combobox.openDropdown()}
          onBlur={() => combobox.closeDropdown()}
          styles={{ input: { fontWeight: 600 } }}
        />
      </Combobox.Target>
      {(filtered.length > 0 || canCreate) && !readOnly && (
        <Combobox.Dropdown>
          {filtered.length > 0 && (
            <Combobox.Header>
              <Text size="xs" c="dimmed">
                Aus Produkten & Leistungen übernehmen
              </Text>
            </Combobox.Header>
          )}
          <Combobox.Options mah={300} style={{ overflowY: 'auto' }}>
            {filtered.map((a) => (
              <Combobox.Option value={String(a.id)} key={a.id}>
                <Group justify="space-between" wrap="nowrap" gap="sm">
                  <Box style={{ minWidth: 0 }}>
                    <Text size="sm" fw={500} truncate>
                      {a.name}
                    </Text>
                    <Text size="xs" c="dimmed" truncate>
                      {a.number} · {a.unit} · {a.tax_rate} % USt
                    </Text>
                  </Box>
                  <Text size="sm" className="tabular" fw={500}>
                    {money(a.price)}
                  </Text>
                </Group>
              </Combobox.Option>
            ))}
            {canCreate && (
              <Combobox.Option value={CREATE_ARTICLE}>
                <Group gap="xs" wrap="nowrap">
                  <IconPlus size={14} />
                  <Text size="sm" truncate>
                    „{value.trim()}“ als neues Produkt / neue Leistung anlegen …
                  </Text>
                </Group>
              </Combobox.Option>
            )}
          </Combobox.Options>
        </Combobox.Dropdown>
      )}
    </Combobox>
  );
}

interface RowProps {
  line: EditorLine;
  index: number;
  pos: number;
  amount: { net: number; gross: number };
  doc: Pick<Document, 'tax_mode' | 'prices_gross' | 'doc_type'>;
  showDiscount: boolean;
  articles: Article[];
  onChange: (patch: Partial<EditorLine>) => void;
  onRemove: () => void;
  onDuplicate: () => void;
  onInsertText: () => void;
  readOnly?: boolean;
}

function LineRow({ line, pos, amount, doc, showDiscount, articles, onChange, onRemove, onDuplicate, onInsertText, readOnly }: RowProps) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: line._key, disabled: readOnly });
  const [showDesc, setShowDesc] = useState(line.description.length > 0);
  const [newArticle, setNewArticle] = useState<string | null>(null);
  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.6 : 1,
    background: isDragging ? 'var(--mantine-color-brand-0)' : undefined,
    position: 'relative',
    zIndex: isDragging ? 2 : undefined,
  };
  const taxEditable = doc.tax_mode === 'standard';
  const applyArticle = (a: Article) => {
    onChange(lineFromArticle(a));
    if (a.description) setShowDesc(true);
  };
  const handle = (
    <ActionIcon variant="transparent" className="drag-handle" {...attributes} {...listeners} aria-label="Position verschieben">
      <IconGripVertical size={16} />
    </ActionIcon>
  );
  const menu = !readOnly && (
    <Menu position="bottom-end">
      <Menu.Target>
        <ActionIcon aria-label="Weitere Aktionen">
          <IconDotsVertical size={16} />
        </ActionIcon>
      </Menu.Target>
      <Menu.Dropdown>
        {line.line_type === 'item' && (
          <Menu.Item leftSection={<IconTextPlus size={16} />} onClick={() => setShowDesc((s) => !s)}>
            {showDesc ? 'Beschreibung ausblenden' : 'Beschreibung hinzufügen'}
          </Menu.Item>
        )}
        <Menu.Item leftSection={<IconCopy size={16} />} onClick={onDuplicate}>
          Duplizieren
        </Menu.Item>
        <Menu.Item leftSection={<IconLetterT size={16} />} onClick={onInsertText}>
          Textzeile darunter einfügen
        </Menu.Item>
        <Menu.Divider />
        <Menu.Item color="red" leftSection={<IconTrash size={16} />} onClick={onRemove}>
          Entfernen
        </Menu.Item>
      </Menu.Dropdown>
    </Menu>
  );

  if (line.line_type === 'text') {
    return (
      <Table.Tr ref={setNodeRef} style={style}>
        <Table.Td>{handle}</Table.Td>
        <Table.Td colSpan={showDiscount ? 7 : 6}>
          <Textarea placeholder="Freitext, z. B. Hinweise zu den folgenden Positionen" value={line.name} onChange={(e) => onChange({ name: e.currentTarget.value })} minRows={1} readOnly={readOnly} />
        </Table.Td>
        <Table.Td>{menu}</Table.Td>
      </Table.Tr>
    );
  }
  if (line.line_type === 'subtotal') {
    return (
      <Table.Tr ref={setNodeRef} style={style}>
        <Table.Td>{handle}</Table.Td>
        <Table.Td colSpan={showDiscount ? 6 : 5}>
          <TextInput value={line.name} placeholder="Zwischensumme" onChange={(e) => onChange({ name: e.currentTarget.value })} readOnly={readOnly} leftSection={<IconSum size={14} />} />
        </Table.Td>
        <Table.Td ta="right">
          <Text fw={650} className="tabular" pt={6}>
            {money(doc.prices_gross ? amount.gross : amount.net)}
          </Text>
        </Table.Td>
        <Table.Td>{menu}</Table.Td>
      </Table.Tr>
    );
  }
  if (line.line_type === 'deduction') {
    return (
      <Table.Tr ref={setNodeRef} style={style}>
        <Table.Td />
        <Table.Td colSpan={showDiscount ? 6 : 5}>
          <Group gap="xs">
            <Badge color="orange" variant="light">
              Abzug
            </Badge>
            <Text size="sm">{line.name}</Text>
            <Text size="xs" c="dimmed">
              netto {money(line.net_amount)} · USt {money(line.tax_amount)}
            </Text>
          </Group>
        </Table.Td>
        <Table.Td ta="right">
          <Text fw={600} className="tabular" c="orange.8">
            {money(line.net_amount + line.tax_amount)}
          </Text>
        </Table.Td>
        <Table.Td />
      </Table.Tr>
    );
  }
  return (
    <Table.Tr ref={setNodeRef} style={style}>
      <Table.Td>
        <Group gap={0} wrap="nowrap">
          {handle}
          <Text size="sm" c="dimmed" w={18} ta="center">
            {pos}
          </Text>
        </Group>
      </Table.Td>
      <Table.Td style={{ minWidth: 220 }}>
        <Stack gap={6}>
          <ArticleNameInput
            value={line.name}
            articles={articles}
            readOnly={readOnly}
            onChange={(v) => onChange({ name: v })}
            onPick={applyArticle}
            onCreate={setNewArticle}
          />
          <ArticleModal
            opened={newArticle !== null}
            onClose={() => setNewArticle(null)}
            article={null}
            initial={{
              name: newArticle ?? '',
              // What was already entered on the line carries over.
              ...(line.unit_price > 0 ? { price: line.unit_price, unit: line.unit, tax_rate: line.tax_rate } : {}),
            }}
            onSaved={applyArticle}
          />
          {(showDesc || line.description) && (
            <Textarea placeholder="Beschreibung (optional)" size="xs" minRows={1} value={line.description} onChange={(e) => onChange({ description: e.currentTarget.value })} readOnly={readOnly} />
          )}
        </Stack>
      </Table.Td>
      <Table.Td w={92}>
        <DecimalInput value={line.quantity} onChange={(v) => onChange({ quantity: v })} readOnly={readOnly} aria-label="Menge" />
      </Table.Td>
      <Table.Td w={104}>
        <Autocomplete data={UNITS} value={line.unit} onChange={(v) => onChange({ unit: v })} readOnly={readOnly} aria-label="Einheit" comboboxProps={{ width: 140 }} />
      </Table.Td>
      <Table.Td w={128}>
        <PriceInput value={line.unit_price} onChange={(v) => onChange({ unit_price: v })} readOnly={readOnly} />
      </Table.Td>
      {showDiscount && (
        <Table.Td w={84}>
          <DecimalInput value={line.discount_percent} onChange={(v) => onChange({ discount_percent: v })} scale={2} rightSection={<Text size="xs" c="dimmed">%</Text>} readOnly={readOnly} aria-label="Rabatt" />
        </Table.Td>
      )}
      <Table.Td w={92}>
        {taxEditable ? (
          <Select data={TAX_RATES} value={String(line.tax_rate)} onChange={(v) => onChange({ tax_rate: Number(v ?? 19) })} readOnly={readOnly} aria-label="USt" comboboxProps={{ width: 100 }} />
        ) : (
          <Tooltip label="Durch die gewählte Steuerart wird keine Umsatzsteuer berechnet">
            <Text size="sm" c="dimmed" pt={6}>
              0 %
            </Text>
          </Tooltip>
        )}
      </Table.Td>
      <Table.Td w={120} ta="right">
        <Text fw={600} className="tabular" pt={6} style={{ whiteSpace: 'nowrap' }}>
          {money(doc.prices_gross ? amount.gross : amount.net)}
        </Text>
      </Table.Td>
      <Table.Td w={40}>{menu}</Table.Td>
    </Table.Tr>
  );
}

export function LinesEditor({
  doc,
  lines,
  onChange,
  readOnly,
  hidePrices,
}: {
  doc: Pick<Document, 'tax_mode' | 'prices_gross' | 'doc_type'>;
  lines: EditorLine[];
  onChange: (lines: EditorLine[]) => void;
  readOnly?: boolean;
  hidePrices?: boolean;
}) {
  const { data: articleItems } = useArticles();
  const articles = useMemo(() => (articleItems ?? []).map((a) => a.article), [articleItems]);
  const [forceDiscount, setForceDiscount] = useState(false);
  const showDiscount = forceDiscount || lines.some((l) => l.discount_percent);
  const { amounts } = useMemo(() => calculate(doc, lines), [doc, lines]);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }), useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }));

  const update = (i: number, patch: Partial<EditorLine>) => onChange(lines.map((l, j) => (j === i ? { ...l, ...patch } : l)));
  const insert = (i: number, l: EditorLine) => onChange([...lines.slice(0, i), l, ...lines.slice(i)]);
  const onDragEnd = (e: DragEndEvent) => {
    if (!e.over || e.active.id === e.over.id) return;
    const from = lines.findIndex((l) => l._key === e.active.id);
    const to = lines.findIndex((l) => l._key === e.over!.id);
    onChange(arrayMove(lines, from, to));
  };

  let pos = 0;
  return (
    <Stack gap="sm">
      <Box style={{ overflowX: 'auto' }}>
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
          <Table className="lines-table" verticalSpacing={6} horizontalSpacing={6} withRowBorders miw={showDiscount ? 860 : 780}>
            <Table.Thead>
              <Table.Tr>
                <Table.Th w={56}>Pos.</Table.Th>
                <Table.Th>Bezeichnung</Table.Th>
                <Table.Th ta="right">Menge</Table.Th>
                <Table.Th>Einheit</Table.Th>
                <Table.Th ta="right">{hidePrices ? '' : doc.prices_gross ? 'Preis brutto' : 'Preis netto'}</Table.Th>
                {showDiscount && <Table.Th ta="right">Rabatt</Table.Th>}
                <Table.Th>USt</Table.Th>
                <Table.Th ta="right">Gesamt</Table.Th>
                <Table.Th />
              </Table.Tr>
            </Table.Thead>
            <SortableContext items={lines.map((l) => l._key)} strategy={verticalListSortingStrategy}>
              <Table.Tbody>
                {lines.map((line, i) => {
                  if (line.line_type === 'item') pos += 1;
                  return (
                    <LineRow
                      key={line._key}
                      line={line}
                      index={i}
                      pos={pos}
                      amount={amounts[i] ?? { net: 0, gross: 0 }}
                      doc={doc}
                      showDiscount={showDiscount}
                      articles={articles}
                      readOnly={readOnly}
                      onChange={(p) => update(i, p)}
                      onRemove={() => onChange(lines.filter((_, j) => j !== i))}
                      onDuplicate={() => insert(i + 1, { ...line, _key: newKey(), id: 0 })}
                      onInsertText={() => insert(i + 1, emptyLine({ line_type: 'text', quantity: 0, unit: '', tax_rate: 0 }))}
                    />
                  );
                })}
              </Table.Tbody>
            </SortableContext>
          </Table>
        </DndContext>
      </Box>
      {lines.length === 0 && (
        <Text size="sm" c="dimmed" ta="center" py="md">
          Noch keine Positionen. Fügen Sie eine Position hinzu oder wählen Sie ein Produkt aus dem Katalog.
        </Text>
      )}
      {!readOnly && (
        <Group gap="xs">
          <Button variant="light" leftSection={<IconPlus size={16} />} onClick={() => onChange([...lines, emptyLine({ tax_rate: doc.tax_mode === 'standard' ? 19 : 0 })])}>
            Position
          </Button>
          <Menu position="bottom-start" width={380}>
            <Menu.Target>
              <Button variant="default" leftSection={<IconPackage size={16} />}>
                Aus Katalog
              </Button>
            </Menu.Target>
            <Menu.Dropdown mah={360} style={{ overflowY: 'auto' }}>
              {articles.length === 0 && <Menu.Label>Noch keine Produkte oder Leistungen angelegt</Menu.Label>}
              {articles.map((a) => (
                <Menu.Item key={a.id} onClick={() => onChange([...lines, { ...emptyLine(), ...lineFromArticle(a) }])} rightSection={<Text size="xs" className="tabular">{money(a.price)}</Text>}>
                  <Text size="sm">{a.name}</Text>
                  <Text size="xs" c="dimmed">
                    {a.number} · {fmtQty(1)} {a.unit}
                  </Text>
                </Menu.Item>
              ))}
            </Menu.Dropdown>
          </Menu>
          <Button variant="subtle" color="gray" leftSection={<IconLetterT size={16} />} onClick={() => onChange([...lines, emptyLine({ line_type: 'text', quantity: 0, unit: '', tax_rate: 0 })])}>
            Textzeile
          </Button>
          <Button variant="subtle" color="gray" leftSection={<IconSum size={16} />} onClick={() => onChange([...lines, emptyLine({ line_type: 'subtotal', name: 'Zwischensumme', quantity: 0, unit: '', tax_rate: 0 })])}>
            Zwischensumme
          </Button>
          {!showDiscount && (
            <Button variant="subtle" color="gray" onClick={() => setForceDiscount(true)}>
              Rabattspalte
            </Button>
          )}
        </Group>
      )}
    </Stack>
  );
}
