import { Drawer, useWords } from '../../components/ui/VaultWorkspace';
import { EdgeInstallGuide } from './EdgeInstallGuide';
export function PluginGuide({ onClose }: { onClose: () => void }) {
  const w = useWords();
  return <Drawer title={w('浏览器插件', 'Browser extension')} onClose={onClose}>
    <p className="installation-intro">{w('将您在 Edge 浏览器中使用的账号和密码自动保存至 LoginDeck。', 'Automatically save the accounts and passwords you use in Edge to LoginDeck.')}</p>
    <EdgeInstallGuide />
  </Drawer>;
}
