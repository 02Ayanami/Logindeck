import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { tauri } from '../../lib/tauri';
import { useWords } from '../../components/ui/VaultWorkspace';
import english from './help/edge-en.json';
import chinese from './help/edge-zh-CN.json';
let pending: ReturnType<typeof tauri.installEdgeExtension> | undefined;
function prepare() {
  pending ??= tauri.installEdgeExtension().finally(() => { pending = undefined; });
  return pending;
}
export function EdgeInstallGuide() {
  const w = useWords();
  const [copied, setCopied] = useState(false);
  const { i18n } = useTranslation();
  const help = (i18n.resolvedLanguage === 'zh-CN' ? chinese : english).install;
  const [path, setPath] = useState<string>();
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<unknown>();
  const [attempt, setAttempt] = useState(0);
  const [opening, setOpening] = useState(false);

  const explain = (error: unknown) => {
    const code = error instanceof Error && 'code' in error ? String(error.code) : '';
    return code === 'extension.setup_conflict' ? help.conflict : code === 'extension.resources_missing' ? help.missing : code === 'extension.edge_unavailable' ? help.edgeMissing : help.failed;
  };
  useEffect(() => { if (!copied) return; const timer = setTimeout(() => setCopied(false), 2000); return () => clearTimeout(timer); }, [copied]);
  useEffect(() => {
    let live = true;
    setBusy(true); setError(undefined);
    void prepare().then(value => { if (live) setPath(value.extension_path); })
      .catch(reason => { if (live) setError(reason); })
      .finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [attempt]);
  async function open(action: () => Promise<void>) {
    if (opening) return;
    setOpening(true); setError(undefined);
    try { await action(); } catch (reason) { setError(reason); }
    finally { setOpening(false); }
  }
  return <article className="installation-guide">
    <h3>{w('安装教程', 'Installation tutorial')}</h3>
    <section><h4><span>1</span>{w('打开扩展管理页', 'Open extensions')}</h4>
      <p>{w('打开 Edge 扩展页，开启“开发人员模式”。', 'Open Edge extensions and enable Developer mode.')}</p>
      <button disabled={opening} onClick={() => void open(tauri.openEdgeExtensions)}>{w('打开 Edge 扩展页', 'Open Edge extensions')}</button>
      <figure className="edge-help-figure"><img src={new URL('./help/images/developer-mode.png', import.meta.url).href} alt={help.developerAlt} /></figure>
    </section>
    <section><h4><span>2</span>{w('加载插件', 'Load the extension')}</h4>
      <p>{w('点击“加载解压缩的扩展”，选择下方文件夹。', 'Select Load unpacked, then choose the folder below.')}</p>
      <div className="installation-path" aria-busy={busy}>
        {path ? <><label>{w('插件文件夹', 'Extension folder')}<input readOnly value={path} onFocus={event => event.target.select()} /></label>
          <div className="installation-actions"><button className="secondary-button" disabled={opening} onClick={() => void open(async () => { await navigator.clipboard.writeText(path); setCopied(true); })}>{copied ? w('已复制', 'Copied') : w('复制路径', 'Copy path')}</button>
          <button className="secondary-button" disabled={opening} onClick={() => void open(tauri.openEdgeExtensionFolder)}>{w('打开文件夹', 'Open folder')}</button></div>
          <p className="installation-tip">{w('在文件夹选择窗口按 ⌘⇧G，粘贴路径并回车，再确认选择。', 'In the folder picker, press ⌘⇧G, paste the path, press Return, then select the folder.')}</p></>
          : busy ? <p role="status">{w('正在准备插件文件…', 'Preparing extension files…')}</p> : <button onClick={() => setAttempt(value => value + 1)}>{w('重试', 'Retry')}</button>}
      </div>
      <figure className="edge-help-figure"><img src={new URL('./help/images/load-extension.png', import.meta.url).href} alt={help.loadAlt} /></figure>
      <div className="installation-diagram" role="img" aria-label={w('选择 edge-extension 文件夹示意', 'Choose the edge-extension folder illustration')}><span>▱ edge-extension</span><span>✓</span></div>
    </section>
    <section><h4><span>3</span>{w('打开插件', 'Open LoginDeck')}</h4>
      <p>{w('点击 Edge 工具栏中的“扩展”图标，选择 LoginDeck。', 'Select Extensions in the Edge toolbar, then LoginDeck.')}</p>
      <figure className="installation-example"><div className="installation-example__bar">Microsoft Edge <span>⋯　{w('扩展', 'Extensions')}</span></div><div className="installation-diagram"><strong>LoginDeck</strong><span>←</span></div><figcaption>{w('操作示意', 'Illustration')}</figcaption></figure>
    </section>
    <section><h4><span>4</span>{w('检测连接', 'Check connection')}</h4>
      <p>{w('保持 LoginDeck 运行，点击插件中的“检测连接”。显示“已连接 LoginDeck”即可使用。', 'Keep LoginDeck running and select Check connection in the extension. Once Connected to LoginDeck appears, it is ready to use.')}</p>
      <figure className="installation-example"><div className="installation-example__bar">LoginDeck</div><div className="installation-success">✓ {w('已连接 LoginDeck', 'Connected to LoginDeck')}</div><span className="installation-example__button">{w('检测连接', 'Check connection')}</span><figcaption>{w('连接成功示意', 'Connection example')}</figcaption></figure>
    </section>
    {error != null && <p role="alert">{explain(error)}</p>}
  </article>;
}
