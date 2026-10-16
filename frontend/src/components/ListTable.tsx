// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useEffect, useMemo, useState } from 'react';
import { DataTable, type DataTableColumn, type DataTableSortStatus } from 'mantine-datatable';
import { IconDatabaseOff } from '@tabler/icons-react';
import { Stack, Text } from '@mantine/core';

function getValue<T>(record: T, accessor: string): unknown {
  return accessor.split('.').reduce<unknown>((v, k) => (v && typeof v === 'object' ? (v as Record<string, unknown>)[k] : undefined), record);
}

export interface ListTableProps<T> {
  records: T[] | undefined;
  columns: DataTableColumn<T>[];
  idAccessor?: string | ((r: T) => string | number);
  defaultSort?: { column: string; direction: 'asc' | 'desc' };
  onRowClick?: (record: T) => void;
  fetching?: boolean;
  emptyText?: string;
  pageSize?: number;
  selectedRecords?: T[];
  onSelectedRecordsChange?: (r: T[]) => void;
  isRecordSelectable?: (r: T) => boolean;
  minHeight?: number;
  rowColor?: (r: T) => string | undefined;
  noPagination?: boolean;
}

export function ListTable<T>({
  records,
  columns,
  idAccessor = 'id',
  defaultSort,
  onRowClick,
  fetching,
  emptyText = 'Keine Einträge vorhanden',
  pageSize = 25,
  selectedRecords,
  onSelectedRecordsChange,
  isRecordSelectable,
  minHeight = 180,
  rowColor,
  noPagination,
}: ListTableProps<T>) {
  const [sortStatus, setSortStatus] = useState<DataTableSortStatus<T>>({
    columnAccessor: defaultSort?.column ?? '',
    direction: defaultSort?.direction ?? 'desc',
  });
  const [page, setPage] = useState(1);
  const [perPage, setPerPage] = useState(pageSize);

  const sorted = useMemo(() => {
    const list = [...(records ?? [])];
    const acc = String(sortStatus.columnAccessor);
    if (!acc) return list;
    const col = columns.find((c) => c.accessor === acc) as (DataTableColumn<T> & { sortKey?: (r: T) => unknown }) | undefined;
    const keyOf = (r: T) => (col?.sortKey ? col.sortKey(r) : getValue(r, acc));
    list.sort((a, b) => {
      const va = keyOf(a);
      const vb = keyOf(b);
      let cmp: number;
      if (typeof va === 'number' && typeof vb === 'number') cmp = va - vb;
      else cmp = String(va ?? '').localeCompare(String(vb ?? ''), 'de', { numeric: true });
      return sortStatus.direction === 'asc' ? cmp : -cmp;
    });
    return list;
  }, [records, sortStatus, columns]);

  useEffect(() => {
    setPage(1);
  }, [records?.length]);

  const paged = noPagination ? sorted : sorted.slice((page - 1) * perPage, page * perPage);
  const showPagination = !noPagination && sorted.length > perPage;

  const common = {
    className: onRowClick ? "clickable-rows" : undefined,
    records: paged,
    columns,
    idAccessor: idAccessor as never,
    sortStatus,
    onSortStatusChange: setSortStatus,
    highlightOnHover: true,
    withTableBorder: false,
    verticalSpacing: "sm" as const,
    horizontalSpacing: "md" as const,
    fetching: fetching && !records,
    minHeight: records && records.length === 0 ? minHeight : undefined,
    noRecordsIcon: (
      <Stack align="center" gap={4} c="dimmed">
        <IconDatabaseOff size={30} stroke={1.4} />
      </Stack>
    ),
    noRecordsText: emptyText,
    onRowClick: onRowClick ? ({ record }: { record: T }) => onRowClick(record) : undefined,
    selectedRecords,
    onSelectedRecordsChange,
    isRecordSelectable: isRecordSelectable ? (r: T) => isRecordSelectable(r) : undefined,
    rowStyle: rowColor ? (r: T) => ({ backgroundColor: rowColor(r) }) : undefined,
    styles: { header: { background: "var(--app-bg)" } },
  };

  if (!showPagination) return <DataTable<T> {...common} />;
  return (
    <DataTable<T>
      {...common}
      totalRecords={sorted.length}
      recordsPerPage={perPage}
      page={page}
      onPageChange={setPage}
      recordsPerPageOptions={[25, 50, 100]}
      onRecordsPerPageChange={setPerPage}
      recordsPerPageLabel="Einträge pro Seite"
      paginationText={({ from, to, totalRecords }) => `${from}–${to} von ${totalRecords}`}
    />
  );
}

export function TableFooterSum({ label, value }: { label: string; value: string }) {
  return (
    <Text size="sm" fw={650} className="tabular" ta="right">
      <Text span c="dimmed" size="xs" fw={500} mr={6}>
        {label}
      </Text>
      {value}
    </Text>
  );
}
