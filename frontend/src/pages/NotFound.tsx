// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import { useNavigate } from 'react-router';
import { Button } from '@mantine/core';
import { IconMapOff } from '@tabler/icons-react';
import { Empty, Page } from '@/components/ui';

export function NotFoundPage() {
  const navigate = useNavigate();
  return (
    <Page>
      <Empty icon={<IconMapOff size={22} />} title="Seite nicht gefunden" description="Diese Seite existiert nicht oder wurde verschoben." action={<Button onClick={() => navigate('/')}>Zur Übersicht</Button>} />
    </Page>
  );
}
