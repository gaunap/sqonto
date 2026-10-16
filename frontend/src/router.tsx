// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { createBrowserRouter, Navigate } from 'react-router';
import { AppLayout } from './components/AppLayout';
import { DashboardPage } from './pages/Dashboard';
import { OnboardingPage } from './pages/Onboarding';
import { DocumentsPage } from './pages/documents/DocumentsPage';
import { DocumentEditorPage } from './pages/documents/DocumentEditor';
import { DocumentViewPage } from './pages/documents/DocumentView';
import { ContactsPage } from './pages/contacts/ContactsPage';
import { ContactDetailPage } from './pages/contacts/ContactDetailPage';
import { ArticlesPage } from './pages/articles/ArticlesPage';
import { OpenItemsPage } from './pages/receivables/OpenItemsPage';
import { ReceiptsPage } from './pages/receipts/ReceiptsPage';
import { ReceiptEditorPage } from './pages/receipts/ReceiptEditor';
import { BankPage } from './pages/banking/BankPage';
import { CashbookPage } from './pages/cashbook/CashbookPage';
import { JournalPage } from './pages/accounting/JournalPage';
import { AccountsPage } from './pages/accounting/AccountsPage';
import { LedgerPage } from './pages/accounting/LedgerPage';
import { AssetsPage } from './pages/accounting/AssetsPage';
import { UstvaPage } from './pages/tax/UstvaPage';
import { StatementsPage } from './pages/reports/StatementsPage';
import { AnalyticsPage } from './pages/reports/AnalyticsPage';
import { ExportsPage } from './pages/reports/ExportsPage';
import { SettingsPage } from './pages/settings/SettingsPage';
import { LayoutDesignerPage } from './pages/settings/LayoutDesigner';
import { NotFoundPage } from './pages/NotFound';

export const router = createBrowserRouter([
  { path: '/willkommen', element: <OnboardingPage /> },
  {
    path: '/',
    element: <AppLayout />,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: 'angebote', element: <DocumentsPage docType="quote" /> },
      { path: 'auftraege', element: <DocumentsPage docType="order" /> },
      { path: 'lieferscheine', element: <DocumentsPage docType="delivery" /> },
      { path: 'rechnungen', element: <DocumentsPage docType="invoice" /> },
      { path: 'gutschriften', element: <DocumentsPage docType="credit_note" /> },
      { path: 'dokumente/neu', element: <DocumentEditorPage /> },
      { path: 'dokumente/:id', element: <DocumentViewPage /> },
      { path: 'dokumente/:id/bearbeiten', element: <DocumentEditorPage /> },
      { path: 'offene-posten', element: <OpenItemsPage /> },
      { path: 'belege', element: <ReceiptsPage /> },
      { path: 'belege/neu', element: <ReceiptEditorPage /> },
      { path: 'belege/:id', element: <ReceiptEditorPage /> },
      { path: 'kontakte', element: <ContactsPage /> },
      { path: 'kontakte/:id', element: <ContactDetailPage /> },
      { path: 'artikel', element: <ArticlesPage /> },
      { path: 'bank', element: <BankPage /> },
      { path: 'kasse', element: <CashbookPage /> },
      { path: 'buchhaltung', element: <Navigate to="/buchhaltung/journal" replace /> },
      { path: 'buchhaltung/journal', element: <JournalPage /> },
      { path: 'buchhaltung/konten', element: <AccountsPage /> },
      { path: 'buchhaltung/konten/:number', element: <LedgerPage /> },
      { path: 'buchhaltung/anlagen', element: <AssetsPage /> },
      { path: 'steuern/ustva', element: <UstvaPage /> },
      { path: 'berichte', element: <StatementsPage /> },
      { path: 'auswertungen', element: <AnalyticsPage /> },
      { path: 'exporte', element: <ExportsPage /> },
      { path: 'einstellungen', element: <SettingsPage /> },
      { path: 'einstellungen/:tab', element: <SettingsPage /> },
      { path: 'einstellungen/layouts/:id', element: <LayoutDesignerPage /> },
      { path: '*', element: <NotFoundPage /> },
    ],
  },
]);
