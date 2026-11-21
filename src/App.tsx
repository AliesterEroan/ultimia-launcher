import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  Globe, Shield, Terminal, Play, Power, Check, Settings, X, Sparkles,
  FolderOpen, Search, Trash2, Info, LoaderCircle, CircleAlert, RotateCcw, FileText, User, Server,
  Gamepad2, Waypoints, CircuitBoard, Gauge, Undo2, ArrowLeft
} from 'lucide-react';
import ultimiaIcon from './assets/ultimia-icon.png';
import type { ClearReport, InstallSource, LaunchError, LaunchOutcome, LaunchState, LoginStatus, OfficialKey, PersistedSettings, ServerCheckResult, ServerType, SettingsSection, StatusReport, ThemeId } from './types/launcher';
import { EXAMPLE_CUSTOM_ADDRESS, HIDE_DELAY_MS, OFFICIAL_KEYS, OFFICIAL_LABELS, OFFICIAL_SERVER_DEFAULTS, SETTINGS_KEY, settingCheckboxes } from './data/constants';
import { characters } from './data/characters';
import { launchErrors } from './data/launchErrors';
import { loadSettings, persistSettings, validateAddress } from './utils/settings';
import startupGuideImage from './assets/characters/startup-guide.png';

function parseInstall(install: string | null): { source: InstallSource; folder: string } | null {
  if (!install) return null;
  const sep = install.indexOf(' - ');
  const label = sep >= 0 ? install.slice(0, sep) : install;
  const folder = sep >= 0 ? install.slice(sep + 3).trim() : '';
  if (/vfun/i.test(label)) return { source: 'vfun', folder };
  if (/steam/i.test(label)) return { source: 'steam', folder };
  if (folder) return { source: 'custom', folder };
  return null;
}

function formatOutcome(outcome: LaunchOutcome): string {
  const command = outcome.args.map((arg) => (arg.includes(' ') ? `"${arg}"` : arg)).join(' ');
  const parts = [`COMMAND\n${command}`];
  parts.push(`EXIT CODE: ${outcome.exitCode ?? 'unknown'}${outcome.timedOut ? ' (timed out — process was stopped)' : ''}`);
  if (outcome.stdout.trim()) parts.push(`STDOUT\n${outcome.stdout.trim()}`);
  if (outcome.stderr.trim()) parts.push(`STDERR\n${outcome.stderr.trim()}`);
  return parts.join('\n\n');
}

export default function App() {
  const [initial] = useState(loadSettings);

  const [selectedServer, setSelectedServer] = useState<ServerType>(initial.values.selectedServer);
  const [isBooting, setIsBooting] = useState(true);
  const [bridgeReady, setBridgeReady] = useState(false);
  const [artLoaded, setArtLoaded] = useState(0);
  const [bridgeOk, setBridgeOk] = useState(true);

  const [customIp, setCustomIp] = useState(initial.values.customAddress);
  const customIpError = validateAddress(customIp);
  const customIpEmpty = customIp.trim() === '';
  const startBlocked = !bridgeOk || (selectedServer === 'custom' && customIpError !== null);

  const [showLaunchModal, setShowLaunchModal] = useState(false);
  const [launchState, setLaunchState] = useState<LaunchState>('idle');
  const [launchError, setLaunchError] = useState<LaunchError>('launch-failed');
  const [launchDetail, setLaunchDetail] = useState('');
  const [lastOutcome, setLastOutcome] = useState<LaunchOutcome | null>(null);
  const [showLog, setShowLog] = useState(false);
  const [runningServerName, setRunningServerName] = useState('');
  const hideTimer = useRef<number | null>(null);

  const [errorMessage, setErrorMessage] = useState('');
  const [activeCharacter, setActiveCharacter] = useState(0);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [activeSettingsSection, setActiveSettingsSection] = useState<SettingsSection>('installation');
  const [rightTab, setRightTab] = useState<'settings' | 'custom'>('settings');
  const [customTab, setCustomTab] = useState<'emulator' | 'join' | 'dashboard'>('join');
  const [emuStatus, setEmuStatus] = useState<{ running: boolean; note: string } | null>(null);
  const [emuBusy, setEmuBusy] = useState(false);
  const [serverCheck, setServerCheck] = useState<{ state: 'idle' | 'checking' | 'online' | 'offline' | 'error'; text: string }>({ state: 'idle', text: '' });
  const [installSource, setInstallSource] = useState<InstallSource>(initial.values.installSource);
  const [customFolder, setCustomFolder] = useState(initial.values.customFolder);
  const [detectedDir, setDetectedDir] = useState('');
  const [emulatorDir, setEmulatorDir] = useState(initial.values.emulatorDir);
  const [emulatorAddr, setEmulatorAddr] = useState('…');
  const [dashNote, setDashNote] = useState('');
  const [launcherPathOverride, setLauncherPathOverride] = useState(initial.values.launcherPath);
  const [detectState, setDetectState] = useState<'idle' | 'detecting' | 'found' | 'not-found'>('idle');
  const [detectLabel, setDetectLabel] = useState('');

  const [status, setStatus] = useState<StatusReport | null>(null);
  const [statusChecked, setStatusChecked] = useState(false);
  const [login, setLogin] = useState<LoginStatus | null>(null);
  const [appVersion, setAppVersion] = useState('');
  const [clientVersion, setClientVersion] = useState<string | null>(null);

  const [vfunAccount, setVfunAccount] = useState<string | null>(null);
  const [vfunId, setVfunId] = useState('');
  const [vfunPassword, setVfunPassword] = useState('');
  const [vfunBusy, setVfunBusy] = useState(false);
  const [vfunFormError, setVfunFormError] = useState('');
  const [confirmVfunOut, setConfirmVfunOut] = useState(false);

  const [advancedOptions, setAdvancedOptions] = useState({
    skipVersionCheck: initial.values.skipVersionCheck,
  });
  const [officialServers, setOfficialServers] = useState<Record<OfficialKey, string>>(initial.values.officialServers);
  const [theme, setTheme] = useState<ThemeId>(initial.values.theme);
  const character = characters[activeCharacter];

  useEffect(() => {
    let cancelled = false;
    const minimumDisplay = new Promise<void>((resolve) => window.setTimeout(resolve, 1400));
    const preload = Promise.all(
      characters.map(
        ({ image }) =>
          new Promise<void>((resolve) => {
            const asset = new Image();
            const done = () => {
              if (!cancelled) setArtLoaded((count) => count + 1);
              resolve();
            };
            asset.onload = done;
            asset.onerror = done;
            asset.src = image;
          }),
      ),
    );

    void (async () => {
      let loginReport: LoginStatus | null = null;
      let version = '';
      let bridge = true;
      try {
        [loginReport, version] = await Promise.all([
          invoke<LoginStatus>('login_status'),
          invoke<string>('app_version'),
        ]);
      } catch {
        bridge = false;
      }
      if (cancelled) return;
      setBridgeOk(bridge);
      setLogin(loginReport);
      setAppVersion(version);
      setBridgeReady(true);

      if (bridge) {
        // detect_install reads the registry/Steam manifests directly — no
        // SoulLauncher.exe subprocess, so no launcher window can appear.
        void invoke<StatusReport>('detect_install')
          .then(async (report) => {
            if (cancelled) return;
            setStatus(report);
            setStatusChecked(true);
            const entries = (report.installs?.length ? report.installs : report.install ? [report.install] : [])
              .map(parseInstall)
              .filter((d): d is { source: InstallSource; folder: string } => d !== null);
            const detected = entries[0] ?? null;
            if (detected && !initial.existed) {
              setInstallSource(detected.source);
              if (detected.source === 'custom' && detected.folder) setCustomFolder(detected.folder);
              if (detected.folder) setDetectedDir(detected.folder);
            }
            const ver = await invoke<string | null>('client_version', { installDir: detected?.folder || null });
            if (!cancelled) setClientVersion(ver);
          })
          .catch(() => {
            if (!cancelled) setStatusChecked(true);
          });
      } else {
        setStatusChecked(true);
      }

      await Promise.all([preload, minimumDisplay]);
      if (cancelled) return;
      window.setTimeout(() => {
        if (!cancelled) setIsBooting(false);
      }, 400);
    })();

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<string>('game-watch', (event) => {
      void invoke<LoginStatus>('login_status').then(setLogin).catch(() => {});
      if (event.payload === 'not-seen') {
        setLaunchError('game-not-seen');
        setLaunchDetail('');
        setLaunchState('error');
        setShowLaunchModal(true);
      } else {
        setShowLaunchModal(false);
        setShowLog(false);
        setLaunchState('idle');
      }
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const rotation = window.setInterval(() => {
      setActiveCharacter((current) => (current + 1) % characters.length);
    }, 5000);

    return () => window.clearInterval(rotation);
  }, []);

  const buildSettings = (): PersistedSettings => {
    // An address mid-edit may be invalid; keep the last saved good value in storage
    // instead. Read it once rather than re-parsing localStorage for every key.
    let savedOfficial: Partial<Record<OfficialKey, string>> | undefined;
    try {
      savedOfficial = (JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') as Partial<PersistedSettings>).officialServers;
    } catch {
      savedOfficial = undefined;
    }
    return {
      selectedServer,
      installSource,
      customFolder,
      customAddress: customIp.trim(),
      emulatorDir: emulatorDir.trim(),
      launcherPath: launcherPathOverride,
      skipVersionCheck: advancedOptions.skipVersionCheck,
      officialServers: Object.fromEntries(
        OFFICIAL_KEYS.map((key) => {
          const value = officialServers[key].trim();
          const saved = savedOfficial?.[key];
          return [key, value && !validateAddress(value) ? value : saved ?? OFFICIAL_SERVER_DEFAULTS[key]];
        }),
      ) as Record<OfficialKey, string>,
      theme,
    };
  };

  // Untouched first run writes nothing so auto-detect still applies next boot; existing settings are rewritten
  // once in normalised form so stale values dropped by loadSettings leave storage too.
  const lastSaved = useRef(initial.existed ? '' : JSON.stringify(initial.values));
  useEffect(() => {
    const next = buildSettings();
    const serialized = JSON.stringify(next);
    if (serialized === lastSaved.current) return;
    lastSaved.current = serialized;
    persistSettings(next);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedServer, installSource, customFolder, customIp, emulatorDir, launcherPathOverride, advancedOptions, officialServers, theme]);

  // Apply the persisted theme to the document root; the CSS token layer does the rest.
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // Resolve the emulator's ip:port from ultimia.json (LAN IP when bind is "auto").
  useEffect(() => {
    let alive = true;
    void invoke<string>('emulator_address', { emulatorDir: emulatorDir.trim() || null })
      .then((addr) => { if (alive && addr) setEmulatorAddr(addr); })
      .catch(() => {});
    return () => { alive = false; };
  }, [emulatorDir]);

  const openDashboard = async () => {
    setDashNote('starting dashboard…');
    try {
      const st = await invoke<{ running: boolean; spawned: boolean; note: string }>('open_dashboard', {
        emulatorDir: emulatorDir.trim() || null,
      });
      setDashNote(st.note + ' — opening dashboard window');
    } catch (e) {
      setDashNote('dashboard failed — ' + String(e));
    }
  };

  const startEmulator = async () => {
    if (emuBusy) return;
    setEmuBusy(true);
    try {
      const st = await invoke<{ running: boolean; spawned: boolean; note: string }>('emulator_ensure', {
        emulatorDir: emulatorDir.trim() || null,
      });
      setEmuStatus({ running: st.running, note: st.note });
    } catch (e) {
      setEmuStatus({ running: false, note: 'check failed — ' + String(e) });
    } finally {
      setEmuBusy(false);
    }
  };

  const pickServer = (srv: ServerType) => {
    setSelectedServer(srv);
    setErrorMessage('');
    if (srv !== 'custom' && rightTab === 'custom') {
      setIsSettingsOpen(false);
      setRightTab('settings');
    }
  };

  const openCustomPage = () => {
    setSelectedServer('custom');
    setErrorMessage('');
    setCustomTab('join');
    setRightTab('custom');
    setIsSettingsOpen(true);
  };

  const serverNameFor = (srv: string, ip: string) =>
    srv === 'global'
      ? 'Global Server (official)'
      : srv === 'local'
          ? `Local Emulator — ultimia-server (${emulatorAddr})`
          : `Custom Server (${ip})`;

  const serverName = serverNameFor(selectedServer, customIp.trim());

  const launchTo = async (srv: string, ip: string) => {
    if (!bridgeOk || launchState === 'launching') return;
    setErrorMessage('');
    setShowLog(false);

    if (srv === 'custom') {
      const err = validateAddress(ip);
      if (err) {
        setLaunchError('invalid-server');
        setLaunchDetail(err);
        setLastOutcome(null);
        setLaunchState('error');
        setShowLaunchModal(true);
        return;
      }
    }
    if (installSource === 'custom' && !customFolder.trim()) {
      setLaunchError('game-not-found');
      setLaunchDetail('Choose a game folder in Settings → Installation.');
      setLastOutcome(null);
      setLaunchState('error');
      setShowLaunchModal(true);
      return;
    }
    if (srv === 'global') {
      for (const key of OFFICIAL_KEYS) {
        const error = validateAddress(officialServers[key]);
        if (error) {
          setLaunchError('invalid-server');
          setLaunchDetail(`${OFFICIAL_LABELS[key]} (Settings → Advanced): ${error}`);
          setLastOutcome(null);
          setLaunchState('error');
          setShowLaunchModal(true);
          return;
        }
      }
    }

    setRunningServerName(serverNameFor(srv, ip));
    setLaunchState('launching');
    setShowLaunchModal(true);

    try {
      if (srv === 'local') {
        await invoke<{ running: boolean; spawned: boolean; note: string }>('emulator_ensure', {
          emulatorDir: emulatorDir.trim() || null,
        }).catch(() => null);
      }
      const outcome = await invoke<LaunchOutcome>('launch', {
        config: {
          server: srv === 'local' ? emulatorAddr === '…' ? '127.0.0.1:15001' : emulatorAddr : srv === 'custom' ? ip : 'global',
          installSource,
          customFolder: installSource === 'custom' ? customFolder.trim() : null,
          skipVersionCheck: advancedOptions.skipVersionCheck,
          launcherPath: launcherPathOverride.trim() || null,
          officialServers: srv === 'global'
            ? Object.fromEntries(OFFICIAL_KEYS.map((key) => [key, officialServers[key].trim()]))
            : undefined,
          localEmulator: srv === 'local',
        },
      });
      setLastOutcome(outcome);
      if (outcome.timedOut) {
        setLaunchError('timeout');
        setLaunchDetail('SoulLauncher did not finish within 120 seconds and was stopped.');
        setLaunchState('error');
      } else if (outcome.exitCode === 0) {
        setLaunchState('success');
        hideTimer.current = window.setTimeout(() => {
          hideTimer.current = null;
          void invoke('watch_game').catch(() => {});
        }, HIDE_DELAY_MS);
      } else {
        setLaunchError('launch-failed');
        setLaunchDetail(
          outcome.stderr.trim() || outcome.stdout.trim() || `SoulLauncher exited with code ${outcome.exitCode ?? 'unknown'}.`,
        );
        setLaunchState('error');
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (message.startsWith('launcher-missing:')) setLaunchError('launcher-missing');
      else if (message.startsWith('invalid-server:')) setLaunchError('invalid-server');
      else if (message.startsWith('game-not-found:')) setLaunchError('game-not-found');
      else if (message.startsWith('vfun-signin-needed:')) setLaunchError('vfun-signin-needed');
      else if (message.startsWith('vfun-auth-failed:')) setLaunchError('vfun-auth-failed');
      else if (message.startsWith('vfun-network:')) setLaunchError('vfun-network');
      else if (message.startsWith('vfun-extra-verification:')) setLaunchError('vfun-extra-verification');
      else setLaunchError('launch-failed');
      const sep = message.indexOf(': ');
      setLaunchDetail(sep >= 0 ? message.slice(sep + 2) : message);
      setLastOutcome(null);
      setLaunchState('error');
    }
  };

  const handleStart = () => launchTo(selectedServer, customIp.trim());

  const cancelHide = () => {
    if (hideTimer.current !== null) {
      window.clearTimeout(hideTimer.current);
      hideTimer.current = null;
    }
  };

  const closeLaunchModal = () => {
    // On success the auto-hide must survive: dismissing the modal is not a
    // request to keep the launcher in front of the game. Only cancel the
    // pending hide for launching/error states where the user may retry.
    if (launchState !== 'success') cancelHide();
    setShowLaunchModal(false);
    setLaunchState('idle');
    setShowLog(false);
  };

  const handleErrorPrimary = () => {
    setShowLaunchModal(false);
    setShowLog(false);
    if (launchError === 'game-not-found' || launchError === 'launcher-missing') {
      setIsSettingsOpen(true);
      setActiveSettingsSection('installation');
    } else if (launchError === 'invalid-server') {
      setSelectedServer('custom');
    } else if (launchError === 'vfun-signin-needed') {
      setIsSettingsOpen(true);
      setActiveSettingsSection('profile');
      setLaunchState('idle');
    } else if (launchError === 'vfun-extra-verification') {
      setLaunchState('idle');
    } else {
      void handleStart();
    }
  };

  const handleAutoDetect = async () => {
    setDetectState('detecting');
    try {
      const report = await invoke<StatusReport>('detect_install');
      setStatus(report);
      const entries = (report.installs?.length ? report.installs : report.install ? [report.install] : [])
        .map(parseInstall)
        .filter((d): d is { source: InstallSource; folder: string } => d !== null);
      if (entries.length) {
        // Multiple installs can coexist (VFUN + Steam): keep the current
        // selection if it's still detected, otherwise take the first.
        const pick = entries.find((d) => d.source === installSource) ?? entries[0];
        setInstallSource(pick.source);
        if (pick.source === 'custom' && pick.folder) setCustomFolder(pick.folder);
        if (pick.folder) setDetectedDir(pick.folder);
        setDetectLabel(
          entries.length === 1
            ? pick.source === 'vfun' ? 'VFUN INSTALLATION FOUND' : pick.source === 'steam' ? 'STEAM INSTALLATION FOUND' : 'CUSTOM INSTALLATION FOUND'
            : `MULTIPLE INSTALLS — USING ${pick.source === 'vfun' ? 'VFUN' : pick.source === 'steam' ? 'STEAM' : 'CUSTOM'}`,
        );
        setDetectState('found');
      } else {
        setDetectLabel('NO INSTALLATION DETECTED');
        setDetectState('not-found');
      }
    } catch {
      setDetectLabel('DETECTION FAILED — BRIDGE UNAVAILABLE');
      setDetectState('not-found');
    }
  };

  const handleBrowseFolder = async () => {
    try {
      const folder = await invoke<string | null>('pick_folder', { defaultDir: detectedDir || customFolder || null });
      if (folder) setCustomFolder(folder);
    } catch (error) {
      setErrorMessage(`Folder picker failed: ${String(error)}`);
    }
  };

  const handlePickLauncher = async () => {
    try {
      const file = await invoke<string | null>('pick_launcher_file');
      if (file) setLauncherPathOverride(file);
    } catch (error) {
      setErrorMessage(`File picker failed: ${String(error)}`);
    }
  };

  const handleBrowseEmulator = async () => {
    try {
      const folder = await invoke<string | null>('pick_folder', { defaultDir: null });
      if (folder) setEmulatorDir(folder);
    } catch (error) {
      setErrorMessage(`Folder picker failed: ${String(error)}`);
    }
  };

  const refreshLogin = async () => {
    try {
      setLogin(await invoke<LoginStatus>('login_status'));
    } catch {
      setLogin(null);
    }
    try {
      setVfunAccount(await invoke<string | null>('vfun_account'));
    } catch {
      setVfunAccount(null);
    }
  };

  const refreshProfile = async () => {
    await refreshLogin();
    try {
      setStatus(await invoke<StatusReport>('detect_install'));
    } catch {
      // Keep the last report; login_status above already reflects the files on disk.
    }
  };

  useEffect(() => {
    if (isSettingsOpen && activeSettingsSection === 'profile' && bridgeOk) void refreshLogin();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isSettingsOpen, activeSettingsSection]);

  // Reachability probe for the join address. Runs on a debounce while typing,
  // then keeps re-checking every 5s while the custom page is open so the
  // online/offline badge never goes stale. The backend cross-checks local
  // addresses against Ultimiadash, so the badge agrees with the dashboard.
  const checkSeq = useRef(0);
  const runServerCheck = useCallback(() => {
    if (customIpEmpty || customIpError !== null || !bridgeOk) {
      setServerCheck({ state: 'idle', text: '' });
      return;
    }
    setServerCheck({ state: 'checking', text: 'CHECKING SERVER…' });
    const seq = ++checkSeq.current;
    invoke<ServerCheckResult>('check_server', { address: customIp.trim() })
      .then((r) => {
        if (seq !== checkSeq.current) return;
        const via = r.viaDashboard ? ' · via Ultimiadash' : '';
        setServerCheck(
          r.reachable
            ? { state: 'online', text: `SERVER ONLINE — ${r.latencyMs ?? '?'} ms${r.port ? ` on :${r.port}` : ''}${via}` }
            : { state: 'offline', text: `SERVER OFFLINE — ${r.error ?? 'unreachable'}${via}` }
        );
      })
      .catch((e) => {
        if (seq !== checkSeq.current) return;
        setServerCheck({ state: 'error', text: `CHECK FAILED — ${String(e)}` });
      });
  }, [customIp, customIpEmpty, customIpError, bridgeOk]);

  useEffect(() => {
    const timer = window.setTimeout(runServerCheck, 700);
    return () => window.clearTimeout(timer);
  }, [runServerCheck]);

  const customPageOpen = isSettingsOpen && rightTab === 'custom';
  useEffect(() => {
    if (!customPageOpen) return;
    const id = window.setInterval(() => {
      if (!customIpEmpty && customIpError === null && bridgeOk) runServerCheck();
    }, 5000);
    return () => window.clearInterval(id);
  }, [customPageOpen, runServerCheck, customIpEmpty, customIpError, bridgeOk]);

  const handleVfunSignIn = async () => {
    if (vfunBusy || !vfunId.trim() || !vfunPassword) return;
    setVfunBusy(true);
    setVfunFormError('');
    try {
      const account = await invoke<string>('vfun_sign_in', {
        userId: vfunId.trim(),
        password: vfunPassword,
      });
      setVfunAccount(account);
      setVfunId('');
      setVfunPassword('');
      await refreshLogin();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const sep = message.indexOf(': ');
      setVfunFormError(sep >= 0 ? message.slice(sep + 2) : message);
      setVfunPassword('');
    }
    setVfunBusy(false);
  };

  const handleVfunSignOut = async () => {
    if (vfunBusy) return;
    setVfunBusy(true);
    setVfunFormError('');
    try {
      await invoke<ClearReport>('vfun_sign_out');
      setVfunAccount(null);
      await refreshLogin();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const sep = message.indexOf(': ');
      setVfunFormError(sep >= 0 ? message.slice(sep + 2) : message);
    }
    setVfunBusy(false);
    setConfirmVfunOut(false);
  };

  const handleQuit = () => {
    if (bridgeOk) void invoke('quit_app');
  };

  const effectiveLauncherPath = launcherPathOverride.trim() || status?.launcherPath || '';
  const notFoundOrChecking = statusChecked ? 'Not found' : 'Checking…';
  const bootProgress = Math.round((bridgeReady ? 24 : 0) + (artLoaded / characters.length) * 76);
  const bootStage = !bridgeReady ? 'CONNECTING TO LAUNCHER CORE' : artLoaded < characters.length ? 'LOADING CHARACTER ART' : 'READY';

  if (isBooting) {
    return (
      <div className="h-screen w-screen bg-transparent text-white overflow-hidden select-none">
        <div className="relative w-full h-full overflow-hidden bg-[radial-gradient(circle_at_50%_32%,var(--surface-root-a)_0%,var(--surface-root-b)_44%,var(--surface-root-c)_76%)] rounded-2xl border border-orange-500/20">
          <div className="absolute inset-0 opacity-20 bg-[linear-gradient(rgba(249,115,22,0.08)_1px,transparent_1px),linear-gradient(90deg,rgba(249,115,22,0.08)_1px,transparent_1px)] bg-[size:48px_48px]" />
          <div className="absolute -top-44 left-1/2 -translate-x-1/2 w-[620px] h-[620px] rounded-full border border-orange-500/10" />
          <div className="absolute -top-32 left-1/2 -translate-x-1/2 w-[500px] h-[500px] rounded-full border border-dashed border-orange-500/15 startup-orbit" />
          <div className="absolute top-8 left-8 flex items-center gap-3">
            <img src={ultimiaIcon} alt="Ultimia" className="w-9 h-9 object-contain" />
            <div>
              <p className="text-sm font-black tracking-[0.22em]" style={{ fontFamily: 'var(--font-chakra)' }}>ULTIMIA LAUNCHER</p>
              <p className="text-[9px] text-orange-400/70 font-mono tracking-[0.18em]">SOULWORKER CLIENT</p>
            </div>
          </div>

          <div className="relative z-10 h-full flex flex-col items-center justify-center pb-20">
            <div className="relative h-[285px] w-[285px] flex items-center justify-center">
              <div className="absolute inset-7 rounded-full bg-orange-500/15 blur-[45px] animate-pulse" />
              <div className="absolute inset-5 rounded-full border border-orange-400/20" />
              <img
                src={startupGuideImage}
                alt="SoulWorker startup guide"
                className="startup-mascot relative z-10 h-[270px] w-auto object-contain drop-shadow-[0_18px_28px_rgba(249,115,22,0.28)]"
              />
            </div>
            <h1 className="mt-1 text-2xl font-black tracking-[0.16em]" style={{ fontFamily: 'var(--font-chakra)' }}>
              PREPARING SOULWORKER
            </h1>
            <p className="mt-1 text-[10px] text-orange-300/70 font-mono tracking-[0.2em]">
              {bootStage}
            </p>
          </div>

          <div className="absolute z-20 left-1/2 -translate-x-1/2 bottom-12 w-[min(620px,calc(100%-64px))]">
            <div className="mb-2 flex items-center justify-between text-[10px] font-bold font-mono tracking-[0.14em]">
              <span className="text-gray-500">SYSTEM LOAD</span>
              <span className="text-orange-300">{bootProgress}%</span>
            </div>
            <div
              className="relative h-3 rounded-full border border-orange-500/35 bg-black/70 shadow-[0_0_22px_rgba(249,115,22,0.16)]"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={bootProgress}
            >
              <div className="progress-glow absolute inset-[2px]">
                <div className="progress-fill rounded-full" style={{ '--progress': `${bootProgress}%` } as CSSProperties} />
              </div>
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="relative h-screen w-screen bg-transparent text-white overflow-hidden select-none">
      <div className="relative z-10 w-full h-full bg-[var(--surface-card)] border border-white/10 rounded-2xl overflow-hidden flex flex-col">
        <header data-tauri-drag-region className="h-[52px] shrink-0 bg-[var(--surface-header)] border-b border-white/10 px-5 flex items-center justify-between">
          <div className="flex items-center gap-3" data-tauri-drag-region>
            <img src={ultimiaIcon} alt="Ultimia" className="w-8 h-8 object-contain" />
            <div>
              <div className="flex items-center gap-2">
                <span className="text-base font-extrabold tracking-widest text-white" style={{ fontFamily: 'var(--font-chakra)' }}>
                  ULTIMIA LAUNCHER
                </span>
                <span className="text-[11px] px-2 py-0.5 rounded bg-orange-500/20 text-orange-400 font-bold border border-orange-500/30">
                  SOULWORKER
                </span>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => {
                if (isSettingsOpen) setIsSettingsOpen(false);
                else {
                  setRightTab('settings');
                  setIsSettingsOpen(true);
                }
              }}
              className={`w-9 h-9 rounded-lg border flex items-center justify-center transition-all cursor-pointer ${
                isSettingsOpen
                  ? 'border-orange-500/60 bg-orange-500/15 text-orange-400 shadow-[0_0_16px_rgba(249,115,22,0.22)]'
                  : 'border-white/10 bg-black/30 text-gray-400 hover:text-orange-400 hover:border-orange-500/50 hover:bg-orange-500/10'
              }`}
              aria-label={isSettingsOpen ? 'Return to launcher' : 'Open settings'}
              title={isSettingsOpen ? 'Return to launcher' : 'Settings'}
            >
              <Settings className={`w-[18px] h-[18px] transition-transform duration-500 ${isSettingsOpen ? 'rotate-90' : ''}`} />
            </button>
          </div>
        </header>

        {!bridgeOk && (
          <div className="shrink-0 px-5 py-1.5 bg-amber-500/15 border-b border-amber-500/30 text-amber-300 text-[10px] font-mono">
            The launcher core is not responding, so START is disabled. Close and reopen the launcher.
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-hidden">
          <div className={`flex w-[200%] h-full transition-transform duration-700 ease-[cubic-bezier(0.16,1,0.3,1)] ${isSettingsOpen ? '-translate-x-1/2' : 'translate-x-0'}`}>
            <div className="w-1/2 h-full shrink-0 grid grid-cols-1 lg:grid-cols-12">
              <div className="lg:col-span-7 relative overflow-hidden p-5 lg:p-6 flex flex-col justify-between">
                <div className="absolute -left-24 top-12 w-[430px] h-[430px] rounded-full bg-orange-600/16 blur-[105px] pointer-events-none" />
                <div className="relative z-10">
                  <div className="mb-4">
                    <div className="flex items-center gap-2 text-xs font-bold text-orange-400 uppercase tracking-widest font-mono">
                      <Sparkles className="w-3.5 h-3.5" />
                      <span>SELECT GAME SERVER</span>
                    </div>
                    <h1 className="text-2xl font-black text-white tracking-wide mt-0.5" style={{ fontFamily: 'var(--font-chakra)' }}>
                      ULTIMIA LAUNCHER
                    </h1>
                  </div>

                  <div className="space-y-2">
                    <button
                      type="button"
                      onClick={() => pickServer('global')}
                      className={`server-option w-full p-3 rounded-xl border text-left transition-all flex items-center justify-between cursor-pointer ${
                        selectedServer === 'global'
                          ? 'server-option-active bg-gradient-to-r from-orange-950/70 to-[#21150e] border-orange-500/70 shadow-[0_0_20px_rgba(249,115,22,0.28)] text-white'
                          : 'bg-black/30 border-white/10 hover:border-white/20 text-gray-400 hover:text-gray-200'
                      }`}
                    >
                      <div className="flex items-center gap-3.5">
                        <div className={`p-2.5 rounded-lg border ${
                          selectedServer === 'global'
                            ? 'bg-orange-500/20 border-orange-500/40 text-orange-400'
                            : 'bg-white/5 border-white/10 text-gray-400'
                        }`}>
                          <Globe className="w-5 h-5" />
                        </div>
                        <div>
                          <div className="flex items-center gap-2">
                            <span className="font-bold text-sm tracking-wide text-white" style={{ fontFamily: 'var(--font-chakra)' }}>
                              GLOBAL SERVER
                            </span>
                            <span className={`text-[10px] px-1.5 py-0.5 rounded font-mono ${
                              selectedServer === 'global' ? 'bg-orange-500/20 text-orange-400' : 'bg-white/5 text-gray-500'
                            }`}>
                              OFFICIAL
                            </span>
                          </div>
                          <p className="text-xs text-gray-400 mt-0.5">
                            {installSource === 'steam'
                              ? 'Official servers — launches your Steam client directly'
                              : 'Official servers — VFUN sign-in, starts the game directly'}
                          </p>
                        </div>
                      </div>

                      <div className={`w-5 h-5 rounded-full border-2 flex items-center justify-center ${
                        selectedServer === 'global'
                          ? 'border-orange-400 bg-orange-500 text-black'
                          : 'border-gray-600'
                      }`}>
                        {selectedServer === 'global' && <Check className="w-3 h-3 stroke-[3]" />}
                      </div>
                    </button>

                    <button
                      type="button"
                      onClick={openCustomPage}
                      className="server-option group w-full p-3 rounded-xl border text-left transition-all flex items-center justify-between cursor-pointer bg-black/30 border-white/10 hover:border-white/20 text-gray-400 hover:text-gray-200"
                    >
                      <div className="flex items-center gap-3.5">
                        <div className="p-2.5 rounded-lg border bg-white/5 border-white/10 text-gray-400">
                          <Gamepad2 className="w-5 h-5" />
                        </div>
                        <div>
                          <div className="flex items-center gap-2">
                            <span className="font-bold text-sm tracking-wide text-white" style={{ fontFamily: 'var(--font-chakra)' }}>
                              PLAY PRIVATE
                            </span>
                            <span className="text-[10px] px-1.5 py-0.5 rounded bg-white/5 text-gray-500 font-mono">
                              CUSTOM PAGE
                            </span>
                          </div>
                          <p className="text-xs text-gray-400 mt-0.5">
                            Emulator, join & dashboard — opens the custom server page
                          </p>
                        </div>
                      </div>

                      <ArrowLeft className="w-4 h-4 text-gray-600 group-hover:text-orange-400 transition-colors rotate-180" />
                    </button>
                  </div>

                  {errorMessage && (
                    <div className="mt-3 p-2.5 rounded-lg bg-red-500/20 border border-red-500/40 text-red-300 text-xs font-mono flex items-center gap-2">
                      <X className="w-4 h-4 shrink-0" />
                      <span>{errorMessage}</span>
                    </div>
                  )}
                </div>

                <div className="relative z-10 pt-3 mt-3 border-t border-white/10 flex items-center gap-3">
                  <button
                    type="button"
                    onClick={handleStart}
                    disabled={startBlocked}
                    className="flex-1 h-12 px-6 rounded-xl bg-gradient-to-r from-red-600 via-orange-500 to-amber-500 hover:from-red-500 hover:to-amber-400 text-white font-black text-base tracking-widest transition-all shadow-[0_0_30px_rgba(239,68,68,0.5)] hover:shadow-[0_0_40px_rgba(245,158,11,0.8)] flex items-center justify-center gap-2.5 cursor-pointer active:scale-98 disabled:opacity-50 disabled:cursor-not-allowed"
                    style={{ fontFamily: 'var(--font-chakra)' }}
                  >
                    <Play className="w-5 h-5 fill-white" />
                    <span>START</span>
                  </button>

                  <button
                    type="button"
                    onClick={handleQuit}
                    className="h-12 px-6 rounded-xl bg-red-950/40 hover:bg-red-900/60 border border-red-500/40 hover:border-red-500/80 text-red-300 hover:text-white font-bold text-sm tracking-wider font-mono transition-all flex items-center justify-center gap-2 cursor-pointer shadow-[0_0_15px_rgba(239,68,68,0.15)] shrink-0"
                  >
                    <Power className="w-4 h-4 text-red-400" />
                    <span>QUIT</span>
                  </button>
                </div>
              </div>

              <div
                className="character-stage lg:col-span-5 relative flex items-center justify-center p-5 bg-gradient-to-b from-[var(--surface-stage-a)]/50 to-[var(--surface-stage-b)] border-t lg:border-t-0 lg:border-l border-white/10 overflow-hidden min-h-0"
                style={{ '--accent': character.accent, '--accent-rgb': character.accentRgb } as CSSProperties}
              >
                <div className="absolute inset-0 pointer-events-none flex items-center justify-center">
                  <div className="character-polygon absolute w-72 h-72 rotate-45 rounded-3xl" />
                  <div className="character-ring absolute w-56 h-56 border-2 border-dashed rotate-12 rounded-2xl" />
                  <div className="character-orbit absolute w-80 h-80 rounded-full border" />
                  <div className="character-glow absolute w-72 h-72 rounded-full blur-[80px]" />
                  <div className="character-slash absolute h-[520px] w-14 -rotate-[28deg]" />
                  <div className="absolute bottom-6 left-6 right-6 flex items-center text-[10px] font-mono text-gray-500 border-t border-white/10 pt-2">
                    <span>{character.className.toUpperCase()} • {character.name.split(' ')[0].toUpperCase()}</span>
                  </div>
                </div>

                <div key={character.name} className="character-enter relative z-10 w-full max-w-[350px] flex flex-col items-center">
                  <img
                    src={character.image}
                    alt={`${character.name} - Soulworker`}
                    className="character-art w-full h-[300px] sm:h-[330px] object-contain"
                  />
                  <div className="character-badge mt-2 px-4 py-1.5 rounded-full bg-black/70 backdrop-blur-md flex items-center gap-2 shadow-lg">
                    <span className="character-dot w-2 h-2 rounded-full" />
                    <span className="text-xs font-bold text-white tracking-widest" style={{ fontFamily: 'var(--font-chakra)' }}>
                      {character.name.toUpperCase()}
                    </span>
                    <span className="character-native text-[10px] font-mono">{character.nativeName}</span>
                  </div>
                </div>

                <div className="absolute top-5 left-5 right-5 z-20 flex items-center justify-between">
                  <span className="text-[10px] font-mono text-gray-500 tracking-[0.2em]">SOULWORKER ROSTER</span>
                  <span className="text-[10px] font-mono text-gray-500">{activeCharacter + 1} / {characters.length}</span>
                </div>
                <div className="absolute bottom-5 z-20 flex gap-1.5" aria-label="Select character">
                  {characters.map((item, index) => (
                    <button
                      key={item.name}
                      type="button"
                      onClick={() => setActiveCharacter(index)}
                      className={`character-nav h-1.5 rounded-full cursor-pointer transition-all duration-300 ${index === activeCharacter ? 'w-7' : 'w-1.5 bg-white/25 hover:bg-white/50'}`}
                      style={index === activeCharacter ? { backgroundColor: character.accent } : undefined}
                      aria-label={`Show ${item.name}`}
                    />
                  ))}
                </div>
              </div>
            </div>

            <section className="w-1/2 h-full shrink-0 bg-[var(--surface-section)] flex min-w-0">
              {rightTab === 'custom' ? (
              <>
              <aside className="w-56 shrink-0 border-r border-white/10 bg-[var(--surface-sidebar)] p-5 flex flex-col">
                <div className="mb-5">
                  <div className="flex items-center gap-2 text-[10px] font-bold text-orange-400 uppercase tracking-widest font-mono">
                    <Terminal className="w-3.5 h-3.5" />
                    Private Play
                  </div>
                  <h2 className="text-xl font-black tracking-wide mt-1" style={{ fontFamily: 'var(--font-chakra)' }}>CUSTOM SERVER</h2>
                </div>
                <nav className="space-y-1.5" aria-label="Custom server sections">
                  {([
                    ['join', 'Join Server', Waypoints],
                    ['emulator', 'Emulator', CircuitBoard],
                    ['dashboard', 'Dashboard', Gauge],
                  ] as const).map(([tab, label, Icon]) => (
                    <button
                      key={tab}
                      type="button"
                      onClick={() => setCustomTab(tab)}
                      className={`w-full h-11 px-3 rounded-lg border flex items-center gap-3 text-xs font-bold transition-all cursor-pointer ${
                        customTab === tab
                          ? 'border-orange-500/50 bg-orange-500/12 text-orange-300 shadow-[inset_3px_0_0_var(--color-orange-500)]'
                          : 'border-transparent text-gray-500 hover:text-gray-200 hover:bg-white/5'
                      }`}
                    >
                      <Icon className="w-4 h-4" />
                      {label}
                    </button>
                  ))}
                </nav>
                <div className="mt-auto pt-4 border-t border-white/10">
                  <button
                    type="button"
                    onClick={() => { setIsSettingsOpen(false); setRightTab('settings'); }}
                    className="w-full h-10 px-3 rounded-lg border border-red-500/25 text-red-400/80 hover:text-red-300 hover:bg-red-500/10 flex items-center gap-3 text-xs font-bold transition-all cursor-pointer"
                  >
                    <Undo2 className="w-4 h-4" />
                    BACK
                  </button>
                </div>
              </aside>

              <div className="flex-1 min-w-0 p-6 flex flex-col">
                <div className="mb-5">
                  <h3 className="text-xl font-black tracking-wide" style={{ fontFamily: 'var(--font-chakra)' }}>
                    {customTab === 'join' && 'JOIN YOUR SERVER'}
                    {customTab === 'emulator' && 'LOCAL EMULATOR'}
                    {customTab === 'dashboard' && 'SERVER DASHBOARD'}
                  </h3>
                  <p className="text-xs text-gray-500 mt-1">
                    {customTab === 'join' && 'Point the game at your own SoulWorker server address.'}
                    {customTab === 'emulator' && 'Start and check the local Rust emulator (ultimia-server).'}
                    {customTab === 'dashboard' && 'Open Ultimiadash — accounts, characters, items and config.'}
                  </p>
                </div>

                <div key={customTab} className="animate-settings-field flex-1 min-h-0 overflow-y-auto space-y-4">
                  {customTab === 'join' && (
                    <>
                      <div className="settings-card">
                        <div className="settings-card-title">
                          <Waypoints className="w-4 h-4" />
                          <span>Server Address</span>
                        </div>
                        <div className="flex gap-2 mb-3">
                          <div className="flex-1 min-w-0">
                            <div className="mb-1 flex items-center justify-between text-[9px] font-mono font-bold tracking-widest">
                              <label htmlFor="custom-page-ip" className="text-amber-400">IP OR IP:PORT</label>
                              <span className={customIpEmpty ? 'text-gray-500' : customIpError ? 'text-amber-300' : 'text-emerald-400'}>
                                {customIpEmpty ? 'NOT SET' : customIpError ? 'CHECK ADDRESS' : 'READY'}
                              </span>
                            </div>
                            <input
                              id="custom-page-ip"
                              type="text"
                              value={customIp}
                              onChange={(e) => setCustomIp(e.target.value)}
                              placeholder={`e.g. ${EXAMPLE_CUSTOM_ADDRESS}`}
                              aria-invalid={!customIpEmpty && customIpError !== null}
                              title={customIpError ?? undefined}
                              className={`w-full h-10 bg-black/30 border rounded-lg px-3 text-sm text-white font-mono placeholder:text-gray-500 focus:outline-none ${
                                !customIpEmpty && customIpError ? 'border-amber-500/60 focus:border-amber-400' : 'border-white/20 focus:border-amber-500'
                              }`}
                            />
                            {customIpError && !customIpEmpty && (
                              <p className="mt-1.5 text-[10px] font-mono text-amber-300">{customIpError}</p>
                            )}
                          </div>
                          <button
                            type="button"
                            onClick={() => { setSelectedServer('custom'); void launchTo('custom', customIp.trim()); }}
                            disabled={startBlocked || customIpEmpty || customIpError !== null}
                            className="self-end h-10 px-5 rounded-lg bg-gradient-to-r from-red-600 via-orange-500 to-amber-500 hover:from-red-500 hover:to-amber-400 text-white font-black text-xs tracking-widest transition-all shadow-[0_0_20px_rgba(239,68,68,0.4)] flex items-center gap-2 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
                            style={{ fontFamily: 'var(--font-chakra)' }}
                          >
                            <Play className="w-4 h-4 fill-white" />
                            PLAY
                          </button>
                        </div>
                        {serverCheck.state !== 'idle' && (
                          <div className="mt-3 flex items-center gap-2 text-[10px] font-mono">
                            {serverCheck.state === 'checking' && <LoaderCircle className="w-3 h-3 animate-spin text-amber-300" />}
                            <span className={
                              serverCheck.state === 'online' ? 'text-emerald-400'
                              : serverCheck.state === 'offline' ? 'text-red-400'
                              : 'text-amber-300'
                            }>
                              {serverCheck.text}
                            </span>
                          </div>
                        )}
                        <p className="text-[10px] font-mono text-gray-500 leading-relaxed">
                          Use the server's LAN IP (e.g. 192.168.x.x) — the game client rejects 127.0.0.1.
                          Default login port is 15001; add :port only if your server uses another.
                        </p>
                      </div>
                    </>
                  )}

                  {customTab === 'emulator' && (
                    <>
                      <div className="settings-card">
                        <div className="settings-card-title">
                          <CircuitBoard className="w-4 h-4" />
                          <span>Emulator Status</span>
                        </div>
                        <div className="space-y-2 text-xs font-mono">
                          <div className="flex justify-between">
                            <span className="text-gray-500">ADDRESS</span>
                            <span className="text-orange-300">{emulatorAddr}</span>
                          </div>
                          <div className="flex justify-between">
                            <span className="text-gray-500">STATUS</span>
                            <span className={emuStatus?.running ? 'text-emerald-400' : 'text-gray-400'}>
                              {emuStatus ? (emuStatus.running ? 'RUNNING' : 'NOT RUNNING') : 'NOT CHECKED'}
                            </span>
                          </div>
                        </div>
                        {emuStatus && (
                          <p className="mt-3 text-[10px] font-mono text-gray-500 leading-relaxed">{emuStatus.note}</p>
                        )}
                        <div className="mt-4 flex gap-2 justify-end">
                          <button
                            type="button"
                            onClick={() => void startEmulator()}
                            disabled={emuBusy}
                            className="h-10 px-4 rounded-lg border border-orange-500/40 bg-orange-500/10 text-orange-300 hover:bg-orange-500/20 flex items-center gap-2 text-xs font-bold cursor-pointer disabled:opacity-40"
                          >
                            {emuBusy ? <LoaderCircle className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
                            {emuBusy ? 'CHECKING…' : 'START EMULATOR'}
                          </button>
                          <button
                            type="button"
                            onClick={() => { pickServer('local'); }}
                            className="h-10 px-4 rounded-lg border border-white/15 bg-white/5 text-gray-300 hover:bg-white/10 flex items-center gap-2 text-xs font-bold cursor-pointer"
                          >
                            USE LOCAL EMULATOR
                          </button>
                        </div>
                        <p className="mt-3 text-[10px] font-mono text-gray-500 leading-relaxed">
                          The emulator also auto-starts when you press PLAY with LOCAL EMULATOR selected.
                          If ultimia-server.exe is not found, set its folder in Settings → Installation.
                        </p>
                      </div>
                    </>
                  )}

                  {customTab === 'dashboard' && (
                    <>
                      <div className="settings-card">
                        <div className="settings-card-title">
                          <Gauge className="w-4 h-4" />
                          <span>Ultimiadash</span>
                        </div>
                        <p className="text-xs text-gray-400 leading-relaxed mb-4">
                          Admin panel for your server — accounts, characters, items, quests, server
                          start/stop, config and logs. Opens as its own desktop app.
                        </p>
                        <div className="flex justify-end">
                          <button
                            type="button"
                            onClick={() => void openDashboard()}
                            className="h-10 px-4 rounded-lg border border-orange-500/40 bg-orange-500/10 text-orange-300 hover:bg-orange-500/20 flex items-center gap-2 text-xs font-bold cursor-pointer"
                          >
                            <Gauge className="w-4 h-4" />
                            OPEN DASHBOARD
                          </button>
                        </div>
                        {dashNote && (
                          <p className="mt-3 text-[10px] font-mono text-gray-500 leading-relaxed">{dashNote}</p>
                        )}
                      </div>
                    </>
                  )}
                </div>
              </div>
              </>
              ) : (
              <>
              <aside className="w-56 shrink-0 border-r border-white/10 bg-[var(--surface-sidebar)] p-5 flex flex-col">
                <div className="mb-5">
                  <div className="flex items-center gap-2 text-[10px] font-bold text-orange-400 uppercase tracking-widest font-mono">
                    <Settings className="w-3.5 h-3.5" />
                    Configuration
                  </div>
                  <h2 className="text-xl font-black tracking-wide mt-1" style={{ fontFamily: 'var(--font-chakra)' }}>SETTINGS</h2>
                </div>
                <nav className="space-y-1.5" aria-label="Settings sections">
                  {([
                    ['installation', 'Installation', FolderOpen],
                    ['advanced', 'Advanced', Terminal],
                    ['profile', 'Profile', Shield],
                    ['about', 'About', Info],
                  ] as const).map(([section, label, Icon]) => (
                    <button
                      key={section}
                      type="button"
                      onClick={() => setActiveSettingsSection(section)}
                      className={`w-full h-11 px-3 rounded-lg border flex items-center gap-3 text-xs font-bold transition-all cursor-pointer ${
                        activeSettingsSection === section
                          ? 'border-orange-500/50 bg-orange-500/12 text-orange-300 shadow-[inset_3px_0_0_var(--color-orange-500)]'
                          : 'border-transparent text-gray-500 hover:text-gray-200 hover:bg-white/5'
                      }`}
                    >
                      <Icon className="w-4 h-4" />
                      {label}
                    </button>
                  ))}
                </nav>
              </aside>

              <div className="flex-1 min-w-0 p-6 flex flex-col">
                <div className="mb-5">
                  <h3 className="text-xl font-black tracking-wide" style={{ fontFamily: 'var(--font-chakra)' }}>
                    {activeSettingsSection === 'installation' && 'GAME INSTALLATION'}
                    {activeSettingsSection === 'advanced' && 'ADVANCED OPTIONS'}
                    {activeSettingsSection === 'profile' && 'PROFILE'}
                    {activeSettingsSection === 'about' && 'ABOUT ULTIMIA'}
                  </h3>
                  <p className="text-xs text-gray-500 mt-1">
                    {activeSettingsSection === 'installation' && 'Choose where the launcher should find your game client.'}
                    {activeSettingsSection === 'advanced' && 'Control how the game client starts and reports status.'}
                    {activeSettingsSection === 'profile' && 'Read from Steam and SoulLauncher when you open this page. Nothing is copied, stored or uploaded.'}
                    {activeSettingsSection === 'about' && 'Version details and launcher information.'}
                  </p>
                </div>

                <div key={activeSettingsSection} className="animate-settings-field flex-1 min-h-0 overflow-y-auto">
                  {activeSettingsSection === 'installation' && (
                    <div className="settings-card">
                      <div className="settings-card-title">
                        <FolderOpen className="w-4 h-4" />
                        <span>Game Installation Path</span>
                      </div>
                      <div className="grid grid-cols-3 gap-2 mb-3">
                        {(['steam', 'vfun', 'custom'] as InstallSource[]).map((source) => (
                          <button
                            key={source}
                            type="button"
                            onClick={() => setInstallSource(source)}
                            className={`settings-radio ${installSource === source ? 'settings-radio-active' : ''}`}
                          >
                            <span className="settings-radio-dot">{installSource === source && <span />}</span>
                            {source === 'vfun' ? 'VFun' : source[0].toUpperCase() + source.slice(1)}
                            {source === 'custom' && ' Folder'}
                          </button>
                        ))}
                      </div>
                      <p className="text-[10px] font-mono text-gray-500 mb-3 truncate" title={status?.install ?? undefined}>
                        {status?.installs?.length
                          ? `Detected installs: ${status.installs.join('  ·  ')}`
                          : status?.install
                            ? `Detected install: ${status.install}`
                            : statusChecked
                              ? 'No installation detected yet.'
                              : 'Scanning for installations…'}
                      </p>
                      {installSource === 'custom' && (
                        <div className="flex gap-2 mb-3 animate-settings-field">
                          <div className="flex-1 min-w-0 h-10 rounded-lg bg-black/30 border border-white/10 px-3 flex items-center text-xs font-mono text-gray-400 truncate" title={customFolder || undefined}>
                            {customFolder || 'No custom folder selected'}
                          </div>
                          <button
                            type="button"
                            onClick={handleBrowseFolder}
                            className="h-10 px-3 rounded-lg border border-orange-500/40 bg-orange-500/10 text-orange-300 hover:bg-orange-500/20 flex items-center gap-2 text-xs font-bold cursor-pointer"
                          >
                            <FolderOpen className="w-4 h-4" />
                            BROWSE
                          </button>
                        </div>
                      )}
                      <button
                        type="button"
                        onClick={handleAutoDetect}
                        disabled={detectState === 'detecting'}
                        className={`w-fit h-10 mx-auto px-4 rounded-lg bg-white/5 border border-white/10 hover:border-orange-500/40 text-xs font-bold flex items-center gap-2 transition-all cursor-pointer disabled:cursor-wait ${
                          detectState === 'found' ? 'text-emerald-300' : detectState === 'not-found' ? 'text-red-300' : 'text-gray-300'
                        }`}
                      >
                        {detectState === 'detecting' ? <LoaderCircle className="w-4 h-4 animate-spin text-orange-400" /> : <Search className="w-4 h-4 text-orange-400" />}
                        {detectState === 'detecting' ? 'SCANNING…' : detectState === 'idle' ? 'AUTO-DETECT INSTALLED GAME' : detectLabel}
                      </button>

                      <div className="mt-4 pt-4 border-t border-white/8">
                        <div className="settings-card-title">
                          <FileText className="w-4 h-4" />
                          <span>SoulLauncher.exe</span>
                        </div>
                        <div className="flex gap-2">
                          <div className="flex-1 min-w-0 h-9 rounded-lg bg-black/30 border border-white/10 px-3 flex items-center text-[10px] font-mono text-gray-400 truncate" title={effectiveLauncherPath || undefined}>
                            {effectiveLauncherPath || notFoundOrChecking}
                          </div>
                          <button
                            type="button"
                            onClick={handlePickLauncher}
                            className="h-9 px-3 rounded-lg border border-orange-500/40 bg-orange-500/10 text-orange-300 hover:bg-orange-500/20 flex items-center gap-2 text-[10px] font-bold cursor-pointer shrink-0"
                          >
                            <FolderOpen className="w-3.5 h-3.5" />
                            BROWSE
                          </button>
                          {launcherPathOverride && (
                            <button
                              type="button"
                              onClick={() => setLauncherPathOverride('')}
                              className="h-9 w-9 rounded-lg border border-white/10 bg-black/30 text-gray-400 hover:text-red-300 hover:border-red-500/40 flex items-center justify-center cursor-pointer shrink-0"
                              aria-label="Reset SoulLauncher path override"
                              title="Reset to auto-resolve"
                            >
                              <X className="w-3.5 h-3.5" />
                            </button>
                          )}
                        </div>
                        <p className="text-[9px] text-gray-600 mt-1.5 font-mono">Override used when starting the game. Leave empty to auto-resolve.</p>
                      </div>

                      <div className="settings-card-title mt-4 pt-4 border-t border-white/8">
                        <Server className="w-4 h-4" />
                        <span>Local Emulator (ultimia-server)</span>
                      </div>
                      <p className="text-[9px] text-gray-600 mb-2 font-mono">Folder containing the server checkout (repo root or its run dir). Leave empty to use ./run or ULTIMIA_RUN/ULTIMIA_EXE env vars.</p>
                      <div className="flex gap-2">
                        <div className="flex-1 min-w-0 h-9 rounded-lg bg-black/30 border border-white/10 px-3 flex items-center text-[10px] font-mono text-gray-400 truncate" title={emulatorDir || undefined}>
                          {emulatorDir || 'Auto (env / ./run)'}
                        </div>
                        <button
                          type="button"
                          onClick={handleBrowseEmulator}
                          className="h-9 px-3 rounded-lg border border-orange-500/40 bg-orange-500/10 text-orange-300 hover:bg-orange-500/20 flex items-center gap-2 text-[10px] font-bold cursor-pointer shrink-0"
                        >
                          <FolderOpen className="w-3.5 h-3.5" />
                          BROWSE
                        </button>
                        {emulatorDir && (
                          <button
                            type="button"
                            onClick={() => setEmulatorDir('')}
                            className="h-9 w-9 rounded-lg border border-white/10 bg-black/30 text-gray-400 hover:text-red-300 hover:border-red-500/40 flex items-center justify-center cursor-pointer shrink-0"
                            aria-label="Reset emulator folder"
                            title="Reset to auto-resolve"
                          >
                            <X className="w-3.5 h-3.5" />
                          </button>
                        )}
                      </div>
                      <p className="text-[9px] text-gray-600 mt-1.5 font-mono">Resolves to {emulatorAddr} from ultimia.json.</p>
                    </div>
                  )}

                  {activeSettingsSection === 'advanced' && (
                    <div className="settings-card">
                      <div className="settings-card-title">
                        <Terminal className="w-4 h-4" />
                        <span>Launch Behavior</span>
                      </div>
                      <div className="divide-y divide-white/8">
                        {settingCheckboxes.map(([key, label, description]) => (
                          <label key={key} className="settings-check-row py-3">
                            <button
                              type="button"
                              role="checkbox"
                              aria-checked={advancedOptions[key]}
                              onClick={() => setAdvancedOptions((current) => ({ ...current, [key]: !current[key] }))}
                              className={`settings-checkbox ${advancedOptions[key] ? 'settings-checkbox-active' : ''}`}
                            >
                              {advancedOptions[key] && <Check className="w-3 h-3" />}
                            </button>
                            <span>
                              <span className="block text-xs text-gray-200">{label}</span>
                              {description && <span className="block text-[9px] text-gray-600 mt-0.5">{description}</span>}
                            </span>
                          </label>
                        ))}
                      </div>

                      <div className="settings-card-title mt-4 pt-4 border-t border-white/8">
                        <Globe className="w-4 h-4" />
                        <span>Official Servers</span>
                      </div>
                      <p className="text-[9px] text-gray-600 mb-2 font-mono">VALOFE's Global login endpoints. Defaults work for everyone; edit only if VALOFE changes them.</p>
                      <div className="space-y-2.5">
                        {OFFICIAL_KEYS.map((key) => {
                          const value = officialServers[key];
                          const error = value.trim() ? validateAddress(value) : 'Enter a server address.';
                          const isDefault = value.trim() === OFFICIAL_SERVER_DEFAULTS[key];
                          return (
                            <div key={key}>
                              <div className="mb-1 flex items-center justify-between text-[9px] font-mono font-bold tracking-widest">
                                <label htmlFor={`official-${key}`} className="text-orange-400">{OFFICIAL_LABELS[key].toUpperCase()}</label>
                                <span className="flex items-center gap-2">
                                  <span className={error ? 'text-amber-300' : isDefault ? 'text-gray-500' : 'text-emerald-400'}>
                                    {error ? 'CHECK ADDRESS' : isDefault ? 'DEFAULT' : 'CUSTOM'}
                                  </span>
                                  {!isDefault && (
                                    <button
                                      type="button"
                                      onClick={() => setOfficialServers((current) => ({ ...current, [key]: OFFICIAL_SERVER_DEFAULTS[key] }))}
                                      className="text-gray-500 hover:text-orange-300 cursor-pointer"
                                      aria-label={`Reset ${OFFICIAL_LABELS[key]} to default`}
                                      title="Reset to default"
                                    >
                                      <X className="w-3 h-3" />
                                    </button>
                                  )}
                                </span>
                              </div>
                              <input
                                id={`official-${key}`}
                                type="text"
                                value={value}
                                onChange={(e) => setOfficialServers((current) => ({ ...current, [key]: e.target.value }))}
                                placeholder={OFFICIAL_SERVER_DEFAULTS[key]}
                                aria-invalid={error !== null}
                                title={error ?? undefined}
                                className={`w-full h-9 bg-[var(--surface-input)] border rounded-lg px-3 text-xs text-white font-mono placeholder:text-gray-500 focus:outline-none ${
                                  error ? 'border-amber-500/60 focus:border-amber-400' : 'border-white/20 focus:border-orange-500'
                                }`}
                              />
                            </div>
                          );
                        })}
                      </div>

                      <div className="settings-card-title mt-4 pt-4 border-t border-white/8">
                        <Sparkles className="w-4 h-4" />
                        <span>Appearance</span>
                      </div>
                      <p className="text-[9px] text-gray-500 mb-2 font-mono">Colour theme only — layout and fonts never change. Click a look to apply it instantly.</p>
                      <div
                        className="grid grid-cols-2 gap-2"
                        role="radiogroup"
                        aria-label="Colour theme"
                        onKeyDown={(e) => {
                          if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
                          e.preventDefault();
                          const cards = [...e.currentTarget.querySelectorAll<HTMLButtonElement>('.theme-card')];
                          const i = cards.findIndex((c) => c.getAttribute('aria-checked') === 'true');
                          const next = cards[(i + (e.key === 'ArrowRight' ? 1 : cards.length - 1)) % cards.length];
                          next.focus();
                          next.click();
                        }}
                      >
                        {([
                          ['launcher', 'Midnight', 'Cool slate on near-black — the launcher night look.'],
                          ['dashboard', 'Ember', 'Warm amber on toasted brown — the Ultimia console glow.'],
                        ] as [ThemeId, string, string][]).map(([id, name, desc]) => (
                          <button
                            key={id}
                            type="button"
                            role="radio"
                            aria-checked={theme === id}
                            data-t={id}
                            onClick={() => setTheme(id)}
                            className={`theme-card ${theme === id ? 'theme-card-active' : ''}`}
                          >
                            <span className="theme-prev" aria-hidden="true">
                              <span className="theme-prev-bar"><i className="theme-prev-dot" /><i /><i /></span>
                              <span className="theme-prev-body">
                                <span className="theme-prev-main"><i /><i /><b className="theme-prev-btn" /></span>
                                <span className="theme-prev-side"><i /><i /></span>
                              </span>
                            </span>
                            <span className="theme-card-meta">
                              <b className="theme-card-name">{name}</b>
                              <i className="theme-card-desc">{desc}</i>
                            </span>
                            <span className="theme-card-check" aria-hidden="true">✓</span>
                          </button>
                        ))}
                      </div>
                    </div>
                  )}

                  {activeSettingsSection === 'profile' && (
                    <div className="space-y-3">
                    <div className="settings-card">
                      <div className="settings-card-title">
                        <User className="w-4 h-4" />
                        <span>Steam</span>
                      </div>
                      <div className="p-3 rounded-lg bg-black/25 border border-white/8 flex items-center justify-between gap-3">
                        <span className="text-[10px] text-gray-500 font-mono shrink-0">SIGNED IN AS</span>
                        <span className={`text-xs font-bold truncate ${login?.steamName ? 'text-gray-100' : 'text-gray-500'}`} title={login?.steamName ?? undefined}>
                          {login ? login.steamName ?? "Steam doesn't report a signed-in account" : 'Unavailable'}
                        </span>
                      </div>
                      <p className="text-[9px] text-gray-600 mt-1.5 font-mono">The display name Steam remembers for auto-login. Steam handles its own sign-in.</p>
                    </div>

                    <div className="settings-card">
                      <div className="settings-card-title">
                        <Globe className="w-4 h-4" />
                        <span>VFUN (Global)</span>
                      </div>
                      {vfunAccount ? (
                        <>
                          <div className="p-3 rounded-lg bg-black/25 border border-white/8 flex items-center justify-between gap-3 mb-3">
                            <span className="text-[10px] text-gray-500 font-mono shrink-0">SIGNED IN AS</span>
                            <span className="text-xs font-bold truncate text-gray-100" title={vfunAccount}>{vfunAccount}</span>
                          </div>
                          <button
                            type="button"
                            disabled={vfunBusy}
                            onClick={() => {
                              if (confirmVfunOut) {
                                void handleVfunSignOut();
                              } else {
                                setConfirmVfunOut(true);
                                window.setTimeout(() => setConfirmVfunOut(false), 4000);
                              }
                            }}
                            className={`w-full h-10 rounded-lg border text-[10px] font-bold font-mono flex items-center justify-center gap-2 cursor-pointer disabled:cursor-wait ${
                              confirmVfunOut
                                ? 'border-red-500 bg-red-600 text-white hover:bg-red-500'
                                : 'border-red-500/30 bg-red-500/8 text-red-300 hover:bg-red-500/15'
                            }`}
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                            {vfunBusy ? 'WORKING…' : confirmVfunOut ? 'CONFIRM — SIGN OUT' : 'SIGN OUT'}
                          </button>
                          <p className="text-[9px] text-gray-600 mt-3 font-mono">
                            Ultimia starts SoulWorker.exe directly with this sign-in — SoulLauncher is not involved. Signing out removes the saved token.
                          </p>
                        </>
                      ) : (
                        <>
                          <form
                            className="space-y-2 mb-3"
                            onSubmit={(event) => {
                              event.preventDefault();
                              void handleVfunSignIn();
                            }}
                          >
                            <input
                              type="text"
                              value={vfunId}
                              onChange={(event) => setVfunId(event.target.value)}
                              placeholder="VFUN ID"
                              autoComplete="username"
                              spellCheck={false}
                              aria-label="VFUN ID"
                              className="w-full h-9 bg-[var(--surface-input)] border border-white/20 rounded-lg px-3 text-xs text-white font-mono placeholder:text-gray-500 focus:outline-none focus:border-orange-500"
                            />
                            <input
                              type="password"
                              value={vfunPassword}
                              onChange={(event) => setVfunPassword(event.target.value)}
                              placeholder="Password"
                              autoComplete="current-password"
                              aria-label="VFUN password"
                              className="w-full h-9 bg-[var(--surface-input)] border border-white/20 rounded-lg px-3 text-xs text-white font-mono placeholder:text-gray-500 focus:outline-none focus:border-orange-500"
                            />
                            <button
                              type="submit"
                              disabled={vfunBusy || !vfunId.trim() || !vfunPassword}
                              className="w-full h-10 rounded-lg bg-gradient-to-r from-orange-600 to-amber-600 hover:from-orange-500 hover:to-amber-500 text-white text-[10px] font-bold font-mono tracking-widest flex items-center justify-center gap-2 cursor-pointer disabled:opacity-40 disabled:cursor-wait"
                            >
                              {vfunBusy && <LoaderCircle className="w-3.5 h-3.5 animate-spin" />}
                              {vfunBusy ? 'SIGNING IN…' : 'SIGN IN'}
                            </button>
                          </form>
                          <p className="text-[9px] text-gray-600 font-mono">
                            Your password goes straight to VFUN and is never stored. Only the returned sign-in token is saved, encrypted by Windows for your user account.
                          </p>
                        </>
                      )}
                      {vfunFormError && <p className="text-[10px] font-mono text-red-300 mt-2 break-all">{vfunFormError}</p>}
                    </div>

                    <button
                      type="button"
                      onClick={refreshProfile}
                      className="w-full h-9 rounded-lg border border-white/10 bg-white/5 hover:border-orange-500/40 text-gray-300 text-[10px] font-bold font-mono flex items-center justify-center gap-2 cursor-pointer"
                    >
                      <RotateCcw className="w-3.5 h-3.5" />
                      REFRESH PROFILE
                    </button>
                    </div>
                  )}

                  {activeSettingsSection === 'about' && (
                    <div className="settings-card h-full">
                      <div className="settings-card-title">
                        <Info className="w-4 h-4" />
                        <span>Version Information</span>
                      </div>
                      <div className="space-y-3 text-xs font-mono">
                        <div className="flex justify-between p-3 rounded-lg bg-black/25 border border-white/8"><span className="text-gray-500">Ultimia Launcher</span><span className="text-gray-200">{appVersion ? `v${appVersion}` : '—'}</span></div>
                        <div className="flex justify-between p-3 rounded-lg bg-black/25 border border-white/8"><span className="text-gray-500">Author</span><span className="text-orange-300">AliesterEroan</span></div>
                        <div className="flex justify-between p-3 rounded-lg bg-black/25 border border-white/8"><span className="text-gray-500">Game client (SoulWorker.exe)</span><span className="text-gray-200">{clientVersion ? `v${clientVersion}` : notFoundOrChecking}</span></div>
                        <div className="flex justify-between p-3 rounded-lg bg-black/25 border border-white/8"><span className="text-gray-500">SoulLauncher.exe</span><span className={status?.found ? 'text-emerald-400' : statusChecked ? 'text-red-300' : 'text-gray-400'}>{status?.found ? 'Found' : notFoundOrChecking}</span></div>
                        <div className="flex justify-between p-3 rounded-lg bg-black/25 border border-white/8 gap-3"><span className="text-gray-500 shrink-0">Last server used</span><span className="text-gray-200 truncate" title={status?.server ?? undefined}>{status?.server ?? '—'}</span></div>
                        {status?.launcherPath && (
                          <p className="text-[9px] text-gray-600 truncate" title={status.launcherPath}>{status.launcherPath}</p>
                        )}
                        {status?.error && <p className="text-[10px] text-amber-300">{status.error}</p>}
                        <div className="pt-3 text-[10px] text-gray-600 leading-relaxed">Ultimia Launcher interface. SoulWorker and all related assets belong to their respective owners.</div>
                      </div>
                    </div>
                  )}
                </div>

                <div className="mt-4 pt-3 border-t border-white/10">
                  <span className="text-[10px] font-mono text-gray-600">Changes are saved as you make them.</span>
                </div>
              </div>
              </>
              )}
            </section>
          </div>
        </div>
      </div>

      {showLaunchModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/90 backdrop-blur-md rounded-2xl">
          <div className="bg-[var(--surface-modal)] border-2 border-orange-500/60 rounded-2xl max-w-lg w-full p-6 text-center shadow-[0_0_80px_rgba(245,158,11,0.3)]">
            {showLog && lastOutcome ? (
              <>
                <h2 className="text-lg font-black text-white tracking-widest mb-3" style={{ fontFamily: 'var(--font-chakra)' }}>
                  PROCESS LOG
                </h2>
                <pre className="max-h-64 overflow-auto text-left text-[10px] font-mono text-gray-300 bg-black/60 border border-white/10 rounded-xl p-4 mb-5 whitespace-pre-wrap break-all">
                  {formatOutcome(lastOutcome)}
                </pre>
                <button
                  type="button"
                  onClick={() => setShowLog(false)}
                  className="w-full py-3 rounded-xl border border-white/15 bg-white/5 hover:bg-white/10 text-gray-200 font-bold text-xs font-mono tracking-widest transition-all cursor-pointer"
                >
                  BACK
                </button>
              </>
            ) : launchState === 'error' ? (
              <>
                <div className="w-16 h-16 rounded-full bg-red-500/15 text-red-400 border border-red-500/40 flex items-center justify-center mx-auto mb-4 shadow-[0_0_25px_rgba(239,68,68,0.2)]">
                  <CircleAlert className="w-8 h-8" />
                </div>
                <h2 className="text-xl font-black text-white tracking-wide mb-2" style={{ fontFamily: 'var(--font-chakra)' }}>
                  {launchErrors[launchError].title}
                </h2>
                <p className="text-xs text-gray-400 max-w-sm mx-auto mb-4 leading-relaxed">
                  {launchErrors[launchError].description}
                </p>
                {launchDetail && (
                  <pre className="mb-4 max-h-28 overflow-auto text-left text-[10px] font-mono text-red-200/90 bg-black/50 border border-red-500/25 rounded-lg p-3 whitespace-pre-wrap break-all">
                    {launchDetail}
                  </pre>
                )}
                <div className="flex gap-3">
                  <button
                    type="button"
                    onClick={closeLaunchModal}
                    className="h-11 px-4 rounded-xl border border-white/15 bg-white/5 hover:bg-white/10 text-gray-200 text-xs font-bold tracking-wider cursor-pointer"
                  >
                    BACK
                  </button>
                  {lastOutcome && (
                    <button
                      type="button"
                      onClick={() => setShowLog(true)}
                      className="h-11 px-4 rounded-xl border border-white/10 bg-white/5 text-gray-300 text-xs font-bold flex items-center gap-2 cursor-pointer"
                    >
                      <FileText className="w-4 h-4" />
                      VIEW LOG
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={handleErrorPrimary}
                    className="flex-1 h-11 rounded-xl bg-gradient-to-r from-red-600 to-orange-500 text-white text-xs font-black tracking-wider flex items-center justify-center gap-2 cursor-pointer"
                  >
                    {launchErrors[launchError].primary === 'RETRY' && <RotateCcw className="w-4 h-4" />}
                    {launchErrors[launchError].primary}
                  </button>
                </div>
              </>
            ) : launchState === 'launching' ? (
              <>
                <div className="w-16 h-16 rounded-full bg-orange-500/10 text-orange-400 border border-orange-500/35 flex items-center justify-center mx-auto mb-4 shadow-[0_0_30px_rgba(249,115,22,0.22)]">
                  <LoaderCircle className="w-8 h-8 animate-spin" />
                </div>
                <h2 className="text-xl font-black text-white tracking-widest mb-1" style={{ fontFamily: 'var(--font-chakra)' }}>
                  STARTING SOULWORKER...
                </h2>
                <p className="text-xs text-orange-300/80 font-mono mb-6">
                  {selectedServer === 'global'
                    ? installSource === 'steam'
                      ? 'Starting your Steam SoulWorker.exe directly.'
                      : 'Signing in to VFUN and starting SoulWorker.exe directly.'
                    : 'Starting SoulWorker.exe directly — waiting for it to finish.'}
                </p>
                <div className="relative h-2.5 rounded-full bg-black/60 border border-white/10 mb-2">
                  <div className="progress-glow absolute inset-[2px]">
                    <div className="progress-fill rounded-full" style={{ '--progress': '100%' } as CSSProperties} />
                  </div>
                </div>
                <div className="flex justify-between text-[10px] font-mono text-gray-600">
                  <span className="truncate">{runningServerName}</span>
                  <span className="shrink-0 ml-3">{selectedServer === 'global' ? (installSource === 'steam' ? 'STEAM DIRECT START' : 'VFUN SIGN-IN + DIRECT START') : 'WAITING FOR EXIT CODE'}</span>
                </div>
              </>
            ) : (
              <>
                <div className="w-16 h-16 rounded-full bg-orange-500/10 text-orange-400 border border-orange-500/35 flex items-center justify-center mx-auto mb-4 shadow-[0_0_25px_rgba(245,158,11,0.3)]">
                  <LoaderCircle className="w-8 h-8 animate-spin" />
                </div>
                <h2 className="text-2xl font-black text-white tracking-widest mb-1" style={{ fontFamily: 'var(--font-chakra)' }}>
                  WAITING FOR THE GAME
                </h2>
                <p className="text-xs text-emerald-400 font-mono mb-4">
                  {selectedServer === 'global'
                    ? 'SoulWorker.exe was started with your VFUN sign-in.'
                    : 'SoulLauncher finished without an error (exit code 0).'}
                </p>
                <div className="p-4 rounded-xl bg-black/50 border border-white/10 text-left text-xs font-mono text-gray-300 mb-3 space-y-2">
                  <div className="flex justify-between gap-3"><span className="text-gray-500 shrink-0">Target Server:</span><span className="text-white font-bold truncate" title={runningServerName}>{runningServerName}</span></div>
                  <div className="flex justify-between gap-3"><span className="text-gray-500 shrink-0">Install source:</span><span className="text-amber-400 font-bold">{installSource === 'vfun' ? 'VFun' : installSource === 'steam' ? 'Steam' : customFolder}</span></div>
                </div>
                <p className="text-[10px] text-gray-500 font-mono mb-5 leading-relaxed">
                  {hideTimer.current !== null
                    ? 'Hiding to the tray. The launcher comes back when you close the game, or tells you if SoulWorker.exe never appears.'
                    : 'The launcher stays open. Close this to return.'}
                </p>
                <div className="flex gap-3">
                  {lastOutcome && (
                    <button
                      type="button"
                      onClick={() => {
                        cancelHide();
                        setShowLog(true);
                      }}
                      className="h-12 px-4 rounded-xl border border-white/10 bg-white/5 text-gray-300 text-xs font-bold flex items-center gap-2 cursor-pointer"
                    >
                      <FileText className="w-4 h-4" />
                      VIEW OUTPUT
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={closeLaunchModal}
                    className="flex-1 py-3.5 rounded-xl bg-gradient-to-r from-red-600 to-orange-600 hover:from-red-500 hover:to-orange-500 text-white font-bold text-xs font-mono tracking-widest transition-all cursor-pointer shadow-lg"
                  >
                    RETURN TO LAUNCHER
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}