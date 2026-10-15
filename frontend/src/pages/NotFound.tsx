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
