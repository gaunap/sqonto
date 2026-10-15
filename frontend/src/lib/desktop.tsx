import { useEffect } from 'react';
import { Button, Group, Text } from '@mantine/core';
import { notifications } from '@mantine/notifications';
import { IconFolderOpen } from '@tabler/icons-react';

/**
 * Bridge to the desktop app (`sqonto-desktop`, native window around the same frontend).
 * In the browser all of this is inactive.
 */
declare global {
  interface Window {
    __SQONTO_DESKTOP__?: boolean;
    /** Asked by the desktop window before closing: number of editors with unsaved changes. */
    __sqontoUnsaved?: () => number;
    /** Called by the desktop window after a download was saved. */
    __sqontoDownloaded?: (path: string) => void;
    ipc?: { postMessage: (message: string) => void };
  }
}

export const isDesktop = typeof window !== 'undefined' && !!window.__SQONTO_DESKTOP__;

let unsaved = 0;

/**
 * Warns before the page is left with unsaved changes: when the browser tab is closed or
 * reloaded, and – in the desktop app – when the window is closed.
 */
export function useLeaveWarning(when: boolean) {
  useEffect(() => {
    if (!when) return;
    unsaved += 1;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => {
      unsaved -= 1;
      window.removeEventListener('beforeunload', warn);
    };
  }, [when]);
}

export function installDesktopBridge() {
  window.__sqontoUnsaved = () => unsaved;
  if (!isDesktop) return;
  window.__sqontoDownloaded = (path) => {
    const name = path.split(/[\\/]/).pop() ?? path;
    const id = `download-${path}`;
    notifications.show({
      id,
      color: 'teal',
      title: 'Gespeichert',
      autoClose: 8000,
      message: (
        <>
          <Text size="sm" truncate title={path}>
            {name}
          </Text>
          <Group gap="xs" mt={6}>
            <Button size="compact-xs" variant="light" onClick={() => window.ipc?.postMessage(`open:${path}`)}>
              Öffnen
            </Button>
            <Button size="compact-xs" variant="subtle" leftSection={<IconFolderOpen size={12} />} onClick={() => window.ipc?.postMessage(`reveal:${path}`)}>
              Im Ordner anzeigen
            </Button>
          </Group>
        </>
      ),
    });
  };
}
