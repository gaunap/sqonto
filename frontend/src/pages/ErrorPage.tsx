// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { isRouteErrorResponse, useNavigate, useRouteError } from 'react-router';
import { Button, Code, Group } from '@mantine/core';
import { IconAlertTriangle } from '@tabler/icons-react';
import { Empty, Page } from '@/components/ui';

/** Shown by the router instead of a page that failed to render. */
export function ErrorPage() {
  const error = useRouteError();
  const navigate = useNavigate();
  const message = isRouteErrorResponse(error) ? `${error.status} ${error.statusText}` : error instanceof Error ? error.message : String(error ?? '');
  return (
    <Page>
      <Empty
        icon={<IconAlertTriangle size={22} />}
        title="Hier ist etwas schiefgelaufen"
        description="Die Seite konnte nicht angezeigt werden. Ihre Daten sind davon nicht betroffen – laden Sie die Seite neu oder wechseln Sie zur Übersicht."
        action={
          <Group justify="center">
            <Button onClick={() => window.location.reload()}>Seite neu laden</Button>
            <Button variant="default" onClick={() => navigate('/')}>
              Zur Übersicht
            </Button>
          </Group>
        }
      />
      {message && (
        <Code block maw={720} mx="auto">
          {message}
        </Code>
      )}
    </Page>
  );
}
