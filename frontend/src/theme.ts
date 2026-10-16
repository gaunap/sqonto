// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import {
  ActionIcon,
  Badge,
  Button,
  Card,
  createTheme,
  Drawer,
  Menu,
  Modal,
  NumberInput,
  Paper,
  Select,
  Tabs,
  TextInput,
  Textarea,
  Tooltip,
  type MantineColorsTuple,
} from '@mantine/core';

const brand: MantineColorsTuple = [
  '#eef4ff',
  '#dbe6fe',
  '#bfd3fe',
  '#93b4fd',
  '#6090fa',
  '#3b6ef6',
  '#2552eb',
  '#1d3fd8',
  '#1e36af',
  '#1e318a',
];

// Slightly blue-tinted dark palette, matching the app's own surface tokens in styles.css
// (Mantine's default dark greys clash with the bluish page background).
const dark: MantineColorsTuple = [
  '#c9cbd1',
  '#b4b7bf',
  '#8b909b',
  '#6b707b',
  '#363942',
  '#2d3038',
  '#25272e',
  '#1f2026',
  '#18191e',
  '#121317',
];

export const theme = createTheme({
  primaryColor: 'brand',
  primaryShade: { light: 6, dark: 5 },
  colors: { brand, dark },
  fontFamily: "'Inter Variable', Inter, system-ui, -apple-system, 'Segoe UI', sans-serif",
  fontFamilyMonospace: "'JetBrains Mono', ui-monospace, 'Cascadia Code', Consolas, monospace",
  headings: {
    fontFamily: "'Inter Variable', Inter, system-ui, sans-serif",
    fontWeight: '650',
    sizes: {
      h1: { fontSize: '1.65rem', lineHeight: '1.25' },
      h2: { fontSize: '1.3rem', lineHeight: '1.3' },
      h3: { fontSize: '1.1rem', lineHeight: '1.35' },
      h4: { fontSize: '0.98rem', lineHeight: '1.4' },
    },
  },
  defaultRadius: 'md',
  cursorType: 'pointer',
  respectReducedMotion: true,
  shadows: {
    xs: '0 1px 2px rgba(16, 24, 40, 0.05)',
    sm: '0 1px 3px rgba(16, 24, 40, 0.08), 0 1px 2px rgba(16, 24, 40, 0.04)',
    md: '0 4px 12px rgba(16, 24, 40, 0.08), 0 2px 4px rgba(16, 24, 40, 0.04)',
    lg: '0 12px 28px rgba(16, 24, 40, 0.12), 0 4px 8px rgba(16, 24, 40, 0.06)',
  },
  components: {
    Button: Button.extend({ defaultProps: { radius: 'md' } }),
    ActionIcon: ActionIcon.extend({ defaultProps: { variant: 'subtle', color: 'gray' } }),
    Card: Card.extend({ defaultProps: { withBorder: true, radius: 'lg', padding: 'lg', shadow: 'xs' } }),
    Paper: Paper.extend({ defaultProps: { radius: 'lg' } }),
    Badge: Badge.extend({ defaultProps: { variant: 'light', radius: 'sm' }, styles: { root: { textTransform: 'none', fontWeight: 600 } } }),
    TextInput: TextInput.extend({ defaultProps: { radius: 'md' } }),
    Textarea: Textarea.extend({ defaultProps: { radius: 'md', autosize: true, minRows: 2 } }),
    NumberInput: NumberInput.extend({ defaultProps: { radius: 'md', decimalSeparator: ',', thousandSeparator: '.' } }),
    Select: Select.extend({ defaultProps: { radius: 'md', allowDeselect: false, checkIconPosition: 'right', comboboxProps: { shadow: 'md' } } }),
    Modal: Modal.extend({ defaultProps: { radius: 'lg', centered: true, overlayProps: { backgroundOpacity: 0.45, blur: 2 } } }),
    Drawer: Drawer.extend({ defaultProps: { position: 'right', size: 'lg', overlayProps: { backgroundOpacity: 0.35, blur: 2 } } }),
    Menu: Menu.extend({ defaultProps: { shadow: 'md', radius: 'md', withinPortal: true } }),
    Tabs: Tabs.extend({ defaultProps: { keepMounted: false } }),
    Tooltip: Tooltip.extend({ defaultProps: { withArrow: true, openDelay: 300 } }),
  },
});
