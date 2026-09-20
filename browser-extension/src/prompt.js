let host, shadow, currentId, currentPhase, timer;
export function dismissPrompt() {
  clearTimeout(timer); host?.remove(); host = undefined; shadow = undefined; currentId = undefined; currentPhase = undefined;
}
export function renderPrompt(state) {
  const candidate = state?.candidate;
  if (!candidate) { dismissPrompt(); return; }
  if (candidate.id === currentId && candidate.phase === currentPhase) return;
  dismissPrompt();
  currentId = candidate.id; currentPhase = candidate.phase;
  host = document.createElement('div');
  host.style.cssText = 'position:fixed!important;top:22px!important;right:22px!important;z-index:2147483647!important;width:320px!important;max-width:calc(100vw - 44px)!important;';
  shadow = host.attachShadow({ mode: 'closed' });
  const style = document.createElement('style');
  style.textContent = ':host{all:initial}*{box-sizing:border-box}.card{font:13px -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;color:#26354c;background:white;border:1px solid #e3e8f0;border-left:3px solid #3569df;border-radius:12px;padding:20px;box-shadow:0 10px 40px #14203525}header{display:flex;align-items:center;justify-content:space-between;gap:12px}strong{font-size:14px;font-weight:600}p{margin:12px 0 0;overflow-wrap:anywhere;line-height:1.6}.account{color:#8590a2;margin-top:3px}footer{display:flex;justify-content:flex-end;gap:8px;margin-top:20px}button{font:inherit;border:0;border-radius:6px;padding:8px 12px;cursor:pointer;color:#68758b;background:#f4f6f9}.primary{color:white;background:#3569df}.close{padding:2px 6px;background:none;font-size:19px}button:focus-visible{outline:2px solid #3569df;outline-offset:2px}.brand{font-size:10px;color:#8d98a8;margin-top:15px;letter-spacing:.6px}';
  shadow.append(style);
  const card = document.createElement('section'); card.className = 'card'; card.setAttribute('role', 'dialog');
  const zh = state.locale === 'zh-CN', words = (a, b) => zh ? a : b;
  const header = document.createElement('header'), title = document.createElement('strong');
  title.textContent = candidate.phase === 'pending' ? words('保存到 LoginDeck？', 'Save to LoginDeck?') : candidate.phase === 'saving' ? words('正在保存到 LoginDeck…', 'Saving to LoginDeck…') : candidate.phase === 'failed' ? words('保存未完成', 'Save incomplete') : words('已保存到 LoginDeck', 'Saved to LoginDeck');
  header.append(title); card.append(header);
  const paragraph = (text, className = '') => { const p = document.createElement('p'); p.textContent = text; p.className = className; card.append(p); };
  if (candidate.phase === 'pending') {
    const reject = () => { void chrome.runtime.sendMessage({ type: 'capture.reject', candidateId: candidate.id }); dismissPrompt(); };
    const close = document.createElement('button'); close.className = 'close'; close.textContent = '×'; close.setAttribute('aria-label', words('关闭', 'Close')); close.addEventListener('click', event => { if (event.isTrusted) reject(); }); header.append(close);
    paragraph(new URL(candidate.origin).hostname); paragraph(candidate.username, 'account');
    const footer = document.createElement('footer'), no = document.createElement('button'), yes = document.createElement('button');
    no.textContent = words('暂不保存', 'Not now'); no.addEventListener('click', event => { if (event.isTrusted) reject(); });
    yes.textContent = words('保存', 'Save'); yes.className = 'primary';
    yes.addEventListener('click', async event => {
      if (!event.isTrusted) return;
      renderPrompt({ ...state, candidate: { ...candidate, phase: 'saving', expires: Date.now() + 30000 } });
      try {
        const result = await chrome.runtime.sendMessage({ type: 'capture.confirm', candidateId: candidate.id });
        if (result?.failure) {
          dismissPrompt();
          // A failed write is never presented as a successful save.
        } else renderPrompt(result);
      } catch { dismissPrompt(); }
    });
    footer.append(no, yes); card.append(footer);
  } else if (candidate.phase === 'failed') {
    paragraph(words('请在 LoginDeck 中检查该账号后重试。', 'Check the account in LoginDeck before trying again.'));
  } else if (candidate.phase === 'saved') {
    const result = candidate.result;
    paragraph(result.websiteName + ' · ' + (result.action === 'created_account'
      ? words('已新增', 'Added ') + result.accountDisplayName
      : words('已更新', 'Updated ') + result.accountDisplayName + words('的密码', ' password')));
  }
  paragraph('LoginDeck', 'brand'); shadow.append(card);
  (document.body ?? document.documentElement).append(host);
  timer = setTimeout(dismissPrompt, Math.max(0, candidate.expires - Date.now()));
}
