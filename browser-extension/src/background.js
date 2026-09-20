import { validCandidate, trustedPopup, webOrigin } from './policy.js';
const HOST = 'com.autologin.native';
let native, checking;
let state = { connected: false, enabled: false, locale: 'en', revision: 0 };
const requests = new Map(), candidates = new Map();
function connect() {
  if (native) return native;
  native = chrome.runtime.connectNative(HOST);
  native.onDisconnect.addListener(() => {
    void chrome.runtime.lastError; native = undefined;
    state = { ...state, connected: false, enabled: false };
    for (const entry of requests.values()) { clearTimeout(entry.timer); entry.reject(new Error('host.unavailable')); }
    requests.clear();
    for (const [tabId, candidate] of candidates) { clearTimeout(candidate.timer); candidate.password = ''; candidates.delete(tabId); }
  });
  native.onMessage.addListener(value => {
    if (value?.version !== 1 || typeof value.requestId !== 'string') return;
    const entry = requests.get(value.requestId); if (!entry) return;
    requests.delete(value.requestId); clearTimeout(entry.timer);
    if (value.type === 'error') entry.reject(new Error(value.code)); else entry.resolve(value);
  });
  return native;
}
function request(body, requestId = crypto.randomUUID()) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { requests.delete(requestId); reject(new Error('host.unavailable')); }, 15000);
    requests.set(requestId, { resolve, reject, timer });
    try { connect().postMessage({ version: 1, requestId, body }); }
    catch (reason) { clearTimeout(timer); requests.delete(requestId); reject(reason); }
  });
}
async function checkConnection() {
  checking ??= (async () => {
    try {
      const result = await request({ type: 'status.get' });
      state = { connected: result.enabled === true, enabled: result.enabled === true, revision: result.revision, locale: result.uiLocale === 'zh-CN' ? 'zh-CN' : 'en' };
    } catch { state = { ...state, connected: false, enabled: false }; }
    if (!state.connected) {
      for (const tabId of candidates.keys()) clear(tabId);
    }
    return state;
  })().finally(() => { checking = undefined; });
  return checking;
}
function clear(tabId) {
  const old = candidates.get(tabId);
  if (old) { clearTimeout(old.timer); old.password = ''; candidates.delete(tabId); }
}
function summary(tabId) {
  const candidate = candidates.get(tabId);
  if (!candidate) return null;
  if (Date.now() >= candidate.expires) { clear(tabId); return null; }
  return { id: candidate.id, origin: candidate.origin, username: candidate.username, phase: candidate.phase, result: candidate.result, expires: candidate.expires };
}
function trustedContent(sender) {
  return sender.id === chrome.runtime.id && sender.frameId === 0 && Number.isInteger(sender.tab?.id) && !!webOrigin(sender.url) && webOrigin(sender.url) === webOrigin(sender.tab.url);
}
async function syncScripts() {
  await chrome.scripting.unregisterContentScripts();
  await chrome.scripting.registerContentScripts([{ id: 'logindeck-login', matches: ['https://*/*'], js: ['content.js'], runAt: 'document_start', allFrames: false, persistAcrossSessions: true }]);
}
chrome.runtime.onInstalled.addListener(() => { void syncScripts(); });
chrome.runtime.onStartup.addListener(() => { void syncScripts(); });
chrome.tabs.onRemoved.addListener(clear);
chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (trustedPopup(sender, chrome.runtime.id)) {
    if (message?.type !== 'popup.status') return false;
    void checkConnection().then(respond); return true;
  }
  if (!trustedContent(sender)) return false;
  const tabId = sender.tab.id;
  (async () => {
    if (message?.type === 'capture.status') {
      await checkConnection();
      const existing = candidates.get(tabId);
      if (existing && existing.origin !== webOrigin(sender.url)) clear(tabId);
      return { ...state, candidate: state.connected ? summary(tabId) : null };
    }
    if (message?.type === 'login.candidate') {
      if (!state.connected || !state.enabled || message.revision !== state.revision || !validCandidate(message, sender, chrome.runtime.id)) return {};
      if (candidates.get(tabId)?.phase === 'saving') return {};
      clear(tabId);
      const candidate = { id: crypto.randomUUID(), origin: message.origin, username: message.username, password: message.password, phase: 'pending', expires: Date.now() + 15000 };
      candidate.timer = setTimeout(() => clear(tabId), 15000);
      candidates.set(tabId, candidate);
      message.password = '';
      return { ...state, candidate: summary(tabId) };
    }
    const candidate = candidates.get(tabId);
    if (!candidate || candidate.origin !== webOrigin(sender.url) || candidate.id !== message.candidateId || Date.now() >= candidate.expires) return {};
    if (message.type === 'capture.reject') { clear(tabId); return {}; }
    if (message.type === 'capture.confirm' && candidate.phase === 'pending') {
      clearTimeout(candidate.timer); candidate.phase = 'saving'; candidate.expires = Date.now() + 30000;
      const body = { type: 'account.save', origin: candidate.origin, username: candidate.username, password: candidate.password };
      candidate.password = '';
      try {
        const saved = request(body, candidate.id); body.password = '';
        const result = await saved;
        if (result.type !== 'account.saved') { clear(tabId); return {}; }
        candidate.result = result; candidate.phase = 'saved'; candidate.expires = Date.now() + 4000;
        candidate.timer = setTimeout(() => clear(tabId), 4000);
        return { ...state, candidate: summary(tabId) };
      } catch (error) {
        if (!state.connected || error?.message === 'host.unavailable') {
          clear(tabId);
          return { ...state, failure: true };
        }
        candidate.phase = 'failed';
        candidate.expires = Date.now() + 4000;
        candidate.timer = setTimeout(() => clear(tabId), 4000);
        return { ...state, candidate: summary(tabId) };
      }
    }
    return {};
  })().then(respond).catch(() => respond({ enabled: false }));
  return true;
});
