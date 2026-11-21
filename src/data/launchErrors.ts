import type { LaunchError } from '../types/launcher';

export const launchErrors: Record<LaunchError, { title: string; description: string; primary: string }> = {
  'launcher-missing': {
    title: 'SoulLauncher.exe not found',
    description: 'It must sit next to UltimiaLauncher.exe or in your SoulWorker folder. You can also point to it manually in Settings → Installation.',
    primary: 'OPEN SETTINGS',
  },
  'invalid-server': {
    title: 'Invalid server address',
    description: 'Use a host or IP with an optional port, e.g. 192.168.0.10:15001. Addresses starting with "-" are rejected.',
    primary: 'EDIT SERVER ADDRESS',
  },
  'game-not-found': {
    title: 'Game folder not found',
    description: 'Select a valid SoulWorker installation folder in Settings → Installation.',
    primary: 'OPEN SETTINGS',
  },
  'timeout': {
    title: 'SoulLauncher timed out',
    description: 'It did not finish within 120 seconds and was stopped. Try again, or enable Silent mode if a dialog is blocking it.',
    primary: 'RETRY',
  },
  'launch-failed': {
    title: 'The launch failed',
    description: 'The game could not be started. The detail below has the exact error; the log has the process output when there is one.',
    primary: 'RETRY',
  },
  'game-not-seen': {
    title: 'The game did not start',
    description: 'The launch finished without an error, but SoulWorker.exe did not appear within 60 seconds.',
    primary: 'RETRY',
  },
  'vfun-signin-needed': {
    title: 'VFUN sign-in needed',
    description: 'Global starts SoulWorker.exe directly with your VFUN account. Sign in once in Settings → Profile.',
    primary: 'OPEN PROFILE',
  },
  'vfun-auth-failed': {
    title: 'VFUN rejected the sign-in',
    description: 'Check the VFUN ID and password, then try again.',
    primary: 'RETRY',
  },
  'vfun-network': {
    title: 'VFUN could not be reached',
    description: 'Check your internet connection and try again. The detail below shows the HTTP or Windows error code.',
    primary: 'RETRY',
  },
  'vfun-extra-verification': {
    title: 'VFUN needs extra verification',
    description: 'This account needs an extra verification step Ultimia cannot complete. Use the official VFUN launcher to sign in to this account.',
    primary: 'OK',
  },
};