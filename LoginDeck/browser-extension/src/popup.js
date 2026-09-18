const button = document.getElementById('check');
let chinese = false;
async function check() {
  button.disabled = true;
  button.textContent = chinese ? '正在检测…' : 'Checking…';
  try {
    const state = await chrome.runtime.sendMessage({ type: 'popup.status' });
    chinese = state?.locale === 'zh-CN';
    document.documentElement.lang = chinese ? 'zh-CN' : 'en';
    document.getElementById('title').textContent = chinese ? 'LoginDeck 连接' : 'Connect to LoginDeck';
    document.getElementById('connection').textContent = state?.connected
      ? chinese ? '已连接 LoginDeck' : 'Connected to LoginDeck'
      : chinese ? '请保持 LoginDeck 运行，然后重新检测。' : 'Keep LoginDeck running, then check again.';
  } catch { document.getElementById('connection').textContent = chinese ? '暂未连接 LoginDeck' : 'Not connected to LoginDeck'; }
  finally { button.disabled = false; button.textContent = chinese ? '检测连接' : 'Check connection'; }
}
button.addEventListener('click', check);
void check();
