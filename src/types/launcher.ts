// Mirrors the serde camelCase structs in src-tauri/src/launcher/.

export interface StatusReport {
  launcherPath: string | null;
  found: boolean;
  server: string | null;
  install: string | null;
  error: string | null;
  installs?: string[];
}

export interface LaunchOutcome {
  args: string[];
  exitCode: number | null;
  stdout: string;
  stderr: string;
  timedOut: boolean;
}

export interface ServerCheckResult {
  reachable: boolean;
  latencyMs: number | null;
  port: number | null;
  error: string | null;
  viaDashboard: boolean;
}

export interface LoginStatus {
  steamName: string | null;
}

export interface ClearReport {
  removed: string[];
}

export type ServerType = 'global' | 'custom' | 'local';
export type InstallSource = 'steam' | 'vfun' | 'custom';
export type LaunchState = 'idle' | 'launching' | 'success' | 'error';
export type LaunchError = 'launcher-missing' | 'invalid-server' | 'game-not-found' | 'timeout' | 'launch-failed' | 'game-not-seen' | 'vfun-signin-needed' | 'vfun-auth-failed' | 'vfun-network' | 'vfun-extra-verification';
export type SettingsSection = 'installation' | 'advanced' | 'profile' | 'about';
export type OfficialKey = 'global1' | 'global2' | 'china';
export type ThemeId = 'launcher' | 'dashboard';

export interface PersistedSettings {
  selectedServer: ServerType;
  installSource: InstallSource;
  customFolder: string;
  customAddress: string;
  emulatorDir: string;
  launcherPath: string;
  skipVersionCheck: boolean;
  officialServers: Record<OfficialKey, string>;
  theme: ThemeId;
}