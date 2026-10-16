// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { notifications } from '@mantine/notifications';
import { api, ApiError } from './client';
import type {
  Account,
  ArticleListItem,
  ContactListItem,
  FontList,
  Layout,
  PaymentAccountView,
  Settings,
  SystemStatus,
} from './types';

type Params = Record<string, string | number | boolean | null | undefined>;

export function useApi<T>(path: string | null, params?: Params, options: { enabled?: boolean; keepPrevious?: boolean; refetchInterval?: number } = {}) {
  return useQuery<T, ApiError>({
    queryKey: [path, params ?? {}],
    queryFn: () => api.get<T>(path as string, params),
    enabled: path !== null && (options.enabled ?? true),
    placeholderData: options.keepPrevious ? keepPreviousData : undefined,
    refetchInterval: options.refetchInterval,
  });
}

interface ActionOptions<TResult, TArgs> {
  success?: string | ((result: TResult, args: TArgs) => string);
  onSuccess?: (result: TResult, args: TArgs) => void;
  invalidate?: boolean;
  errorTitle?: string;
}

/** Mutation with automatic cache invalidation and notifications. */
export function useAction<TArgs = void, TResult = unknown>(fn: (args: TArgs) => Promise<TResult>, opts: ActionOptions<TResult, TArgs> = {}) {
  const qc = useQueryClient();
  return useMutation<TResult, ApiError, TArgs>({
    mutationFn: fn,
    onSuccess: (result, args) => {
      if (opts.invalidate !== false) qc.invalidateQueries();
      if (opts.success) {
        const message = typeof opts.success === 'function' ? opts.success(result, args) : opts.success;
        notifications.show({ color: 'teal', message, autoClose: 3000 });
      }
      opts.onSuccess?.(result, args);
    },
    onError: (e) => {
      notifications.show({ color: 'red', title: opts.errorTitle ?? 'Aktion fehlgeschlagen', message: e.message, autoClose: 8000 });
    },
  });
}

export function notifyError(e: unknown, title = 'Fehler') {
  notifications.show({ color: 'red', title, message: e instanceof Error ? e.message : String(e), autoClose: 8000 });
}

export const useStatus = () => useApi<SystemStatus>('/system/status');
export const useSettings = () => useApi<Settings>('/settings');
export const useChart = () => useApi<Account[]>('/accounts');
export const usePaymentAccounts = () => useApi<PaymentAccountView[]>('/payment-accounts');
export const useContacts = (role?: string) => useApi<ContactListItem[]>('/contacts', role ? { role } : undefined);
export const useArticles = () => useApi<ArticleListItem[]>('/articles');
export const useLayouts = () => useApi<Layout[]>('/layouts');
export const useFonts = () => useApi<FontList>('/fonts');
