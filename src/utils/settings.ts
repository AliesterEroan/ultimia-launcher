import type { OfficialKey, PersistedSettings } from '../types/launcher';
import {
  EXAMPLE_CUSTOM_ADDRESS,
  INSTALL_SOURCES,
  OFFICIAL_KEYS,
  OFFICIAL_SERVER_DEFAULTS,
  SERVER_TYPES,
  SETTINGS_KEY,
} from '../data/constants';

// Same rules as validate_server_address in the launcher Rust module.
const ADDRESS_RE = /^(\[[0-9A-Fa-f:]+\]|[A-Za-z0-9](?:[A-Za-z0-9.\-]*[A-Za-z0-9])?)(?::(\d{1,5}))?$/;

export function validateAddress(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return 'Enter a server address.';
  if (trimmed.startsWith('-')) return 'The address must not start with "-".';
  const match = trimmed.match(ADDRESS_RE);
  if (!match) return 'Use a host or IP with an optional port, e.g. 192.168.0.10:15001.';
  if (match[2] !== undefined) {
    const port = Number(match[2]);
    if (port < 1 || port > 65535) return 'Port must be between 1 and 65535.';
  }
  return null;
}

export function loadSettings(): { values: PersistedSettings; existed: boolean } {
  const defaults: PersistedSettings = {
    selectedServer: 'global',
    installSource: 'vfun',
    customFolder: '',
    customAddress: '',
    emulatorDir: '',
    launcherPath: '',
    skipVersionCheck: false,
    officialServers: { ...OFFICIAL_SERVER_DEFAULTS },
    theme: 'launcher',
  };
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (!raw) return { values: defaults, existed: false };
    const parsed = JSON.parse(raw) as Partial<PersistedSettings>;
    const storedServer = SERVER_TYPES.includes(parsed.selectedServer as PersistedSettings['selectedServer']) ? (parsed.selectedServer as PersistedSettings['selectedServer']) : defaults.selectedServer;
    return {
      values: {
        selectedServer: storedServer,
        installSource: INSTALL_SOURCES.includes(parsed.installSource as PersistedSettings['installSource']) ? (parsed.installSource as PersistedSettings['installSource']) : defaults.installSource,
        customFolder: typeof parsed.customFolder === 'string' ? parsed.customFolder : defaults.customFolder,
        // Older builds pre-filled and stored the example address as if the user had entered it.
        customAddress: typeof parsed.customAddress === 'string' && parsed.customAddress.trim() !== EXAMPLE_CUSTOM_ADDRESS ? parsed.customAddress : defaults.customAddress,
        emulatorDir: typeof parsed.emulatorDir === 'string' ? parsed.emulatorDir : defaults.emulatorDir,
        launcherPath: typeof parsed.launcherPath === 'string' ? parsed.launcherPath : defaults.launcherPath,
        skipVersionCheck: parsed.skipVersionCheck === true,
        officialServers: Object.fromEntries(
          OFFICIAL_KEYS.map((key) => {
            const stored = (parsed.officialServers as Partial<Record<OfficialKey, string>> | undefined)?.[key];
            return [key, typeof stored === 'string' && stored.trim() && !validateAddress(stored) ? stored.trim() : OFFICIAL_SERVER_DEFAULTS[key]];
          }),
        ) as Record<OfficialKey, string>,
        theme: parsed.theme === 'dashboard' ? 'dashboard' : 'launcher',
      },
      existed: true,
    };
  } catch {
    return { values: defaults, existed: false };
  }
}

export function persistSettings(values: PersistedSettings) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(values));
  } catch {
    // Storage may be unavailable; settings then live for this session only.
  }
}