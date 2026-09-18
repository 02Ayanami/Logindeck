import { extractLogin } from './detector.js';
import { webOrigin } from './policy.js';
import { renderPrompt, dismissPrompt } from './prompt.js';
// No page messages, injected page scripts, persistent secrets, or cross-origin frame capture.
if (window === window.top && webOrigin(location.href)) {
  let permission={enabled:false}, lastCheck=0, checking=false;
  async function refreshPermission() {
    if (document.hidden) { permission={enabled:false}; return; }
    if (checking) return;
    checking=true;
    try { permission=await chrome.runtime.sendMessage({type:'capture.status'}) ?? {enabled:false}; lastCheck=performance.now(); renderPrompt(permission); }
    catch { permission={enabled:false}; dismissPrompt(); }
    finally { checking=false; }
  }
  document.addEventListener('visibilitychange',()=>{permission={enabled:false};void refreshPermission();});
  void refreshPermission();
  let poll=setInterval(()=>void refreshPermission(),1000);
  window.addEventListener('pagehide',()=>{clearInterval(poll);permission={enabled:false};dismissPrompt();});
  window.addEventListener('pageshow',()=>{clearInterval(poll);poll=setInterval(()=>void refreshPermission(),1000);void refreshPermission();});
  let lastGesture = -Infinity, lastCapture = -Infinity;
  const gesture = event => { if (event.isTrusted) lastGesture = performance.now(); };
  document.addEventListener('pointerdown',gesture,true);
  document.addEventListener('keydown',gesture,true);
  const capture = form => {
    if (!permission.enabled || performance.now()-lastCheck > 2000 || !form || performance.now()-lastGesture > 1500 || performance.now()-lastCapture < 2000) return;
    const candidate = extractLogin(form);
    if (!candidate) return;
    lastCapture = performance.now();
    // The worker retains credentials only until consent or the 15-second expiry.
    const message = {type:'login.candidate',origin:location.origin,revision:permission.revision,...candidate};
    chrome.runtime.sendMessage(message).then(renderPrompt).catch(()=>{});
    candidate.password = ''; message.password = '';
  };
  document.addEventListener('submit',event => { if (event.isTrusted) capture(event.target); },true);
  document.addEventListener('click',event => {
    if (!event.isTrusted) return;
    const button = event.target.closest?.('button,input[type=submit]');
    // Standard submit uses the submit event after browser validity checks. SPA buttons
    // require an explicit login label and still require a real form + unambiguous fields.
    if (button?.type === 'button' && /^(log\s?in|sign\s?in|登录|登入)$/i.test(button.textContent.trim())) capture(button.closest('form'));
  },true);
}
