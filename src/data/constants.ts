import type { InstallSource, OfficialKey, ServerType } from '../types/launcher';

export const EXAMPLE_CUSTOM_ADDRESS = '192.168.0.10:15001';
// VALOFE's official login endpoints, as shipped in the Global hand-off template.
export const OFFICIAL_SERVER_DEFAULTS = {
  global1: '43.159.147.134:10000',
  global2: '43.130.7.93:10000',
  china: '49.233.245.180:10000',
} as const;
export const OFFICIAL_KEYS = Object.keys(OFFICIAL_SERVER_DEFAULTS) as OfficialKey[];
export const OFFICIAL_LABELS: Record<OfficialKey, string> = {
  global1: 'Global server 1',
  global2: 'Global server 2',
  china: 'China server',
};
export const SETTINGS_KEY = 'ultimia-settings';
export const HIDE_DELAY_MS = 2500;

export const SERVER_TYPES: ServerType[] = ['global', 'custom', 'local'];
export const INSTALL_SOURCES: InstallSource[] = ['steam', 'vfun', 'custom'];

export const settingCheckboxes = [
  ['skipVersionCheck', 'Skip version check', 'Bypass the client version gate when connecting to a private server.'],
] as const;