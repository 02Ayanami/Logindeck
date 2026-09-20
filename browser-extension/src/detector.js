import { byteLength } from './policy.js';
export function visible(input) {
  const style = input.ownerDocument.defaultView.getComputedStyle(input);
  return !input.disabled && input.type !== 'hidden' && style.display !== 'none' && style.visibility !== 'hidden' && input.getClientRects().length > 0;
}
export function extractLogin(form) {
  if (!(form instanceof form.ownerDocument.defaultView.HTMLFormElement)) return null;
  const intent = `${form.id} ${form.getAttribute('action') ?? ''} ${[...form.querySelectorAll('button,input[type=submit]')].map(x => x.textContent || x.value).join(' ')}`;
  if (/sign.?up|register|registration|create.?account|reset.?password|change.?password|注册|创建账号|重置密码|修改密码/i.test(intent)) return null;
  const all = [...form.querySelectorAll('input')];
  const passwords = all.filter(x => x.type === 'password' && visible(x));
  // Signup, password change, confirmation, OTP, and payment fields are excluded.
  if (passwords.length !== 1 || all.some(x => /new-password|one-time-code|cc-/.test(x.autocomplete))) return null;
  const password = passwords[0];
  if (/confirm|new.?pass|otp|cvv|cvc|pin/i.test(`${password.name} ${password.id}`)) return null;
  const users = all.filter(x => ['text','email','tel'].includes(x.type) && visible(x) && x.value.trim() && !/otp|cvv|cvc|card|code|token|search/i.test(`${x.name} ${x.id} ${x.autocomplete}`));
  const ranked = users.filter(x => x.autocomplete === 'username');
  const plausible = ranked.length ? ranked : users.filter(x => x.type === 'email' || /user|email|login|account|phone/i.test(`${x.name} ${x.id}`));
  if (plausible.length !== 1 || !password.value || byteLength(plausible[0].value) > 512 || byteLength(password.value) > 4096) return null;
  return { username:plausible[0].value, password:password.value };
}
