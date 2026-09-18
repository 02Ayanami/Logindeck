import { useEffect, useState } from 'react';
import { tauri } from '../../lib/tauri';
import { useWords } from '../../components/ui/VaultWorkspace';

export function PluginConnectionStatus() {
  const w = useWords();
  const [lease, setLease] = useState<number | null>(null);
  const [checked, setChecked] = useState(false);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    let active = true;
    let pending = false;
    async function refresh() {
      setNow(Date.now());
      if (pending) return;
      pending = true;
      try {
        const settings = await tauri.getBrowserCaptureSettings();
        if (active) setLease(settings.enabled ? settings.last_connected_at : null);
      } catch {
        if (active) setLease(null);
      } finally {
        pending = false;
        if (active) setChecked(true);
      }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  const connected = lease !== null && now / 1000 - lease >= -2 && now / 1000 - lease < 6;
  const state = !checked ? 'checking' : connected ? 'connected' : 'disconnected';
  const label = !checked ? w('检测中', 'Checking') : connected ? w('已连接', 'Connected') : w('未连接', 'Disconnected');
  return <span className="plugin-connection" data-state={state} aria-label={w('浏览器插件：', 'Browser extension: ') + label} aria-live="polite"><span className="plugin-connection__dot" aria-hidden="true" />{label}</span>;
}
