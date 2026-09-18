// Development-only visual review with synthetic accounts; never connects to the real vault.
import { mockIPC } from '@tauri-apps/api/mocks';
import { createRoot } from 'react-dom/client';
import { App } from './app/App';
import './app/app.css';
import './app/workspace.css';
const id = (n: number) => '550e8400-e29b-41d4-a716-' + String(n).padStart(12, '0');
const sites = [
  ['GitHub', 'https://github.com', '个人账号', 'hao@example.com'],
  ['GitHub', 'https://github.com', '工作账号', 'work@example.com'],
  ['GitHub', 'https://github.com', '测试账号', 'test@example.com'],
  ['GitHub', 'https://github.com', '账号 4', 'demo@example.com'],
  ['Notion', 'https://notion.so', '个人空间', 'hao@example.com'],
].map(([name, origin, account_name, username], i) => ({ id: id(i), name, url: origin, normalized_origin: origin, account_name, username, notes: '', capture_source: 'manual', created_at: '1', updated_at: '1' }));
const applications = ['QQ', '微信', '飞书'].map((name, i) => ({
  id: id(i + 20), platform: 'macos', display_name: name, platform_application_id: 'com.preview.' + i,
  launch_target: '/Applications/' + name + '.app', alternate_launch_targets: [], version: '1.0', discovery_source: 'automatic', is_present: true,
  last_discovered_at: '1', created_at: '1', updated_at: '1',
  accounts: ['个人账号', '工作账号'].map((display_name, j) => ({ id: id(i * 10 + j + 40), application_id: id(i + 20), display_name, username: j ? 'work@example.com' : '12345678', login_method: 'password', phone: null, auto_submit_enabled: false, last_login_status: null, last_login_at: null, created_at: '1', updated_at: '1' })),
}));
mockIPC((command) => {
  if (command === 'install_edge_extension') return { extension_path: '/Users/demo/Library/Application Support/LoginDeck/edge-extension' };
  if (command === 'open_edge_extensions' || command === 'open_edge_extension_folder') return null;
  if (command === 'get_browser_capture_settings') return { enabled: true, revision: 0, last_connected_at: Math.floor(Date.now() / 1000) };
  if (command === 'get_settings') return { ui_locale: 'zh-CN' };
  if (command === 'list_websites') return sites;
  if (command === 'list_applications') return applications;
  if (command === 'get_scan_status') return { id: 0, phase: 'idle', started_at: null, count: null, error: null };
  if (command === 'next_account_number') return 5;
  if (command === 'get_application_icon') return null;
  throw { code: 'platform.unsupported', params: {} };
});
createRoot(document.getElementById('root')!).render(<App />);
