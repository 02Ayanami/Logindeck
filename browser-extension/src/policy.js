export const byteLength = value => new TextEncoder().encode(value).length;
export function webOrigin(value) {
  try { const u = new URL(value); return u.protocol === 'https:' && !u.username && !u.password ? u.origin : null; } catch { return null; }
}
export function validCandidate(message, sender, extensionId) {
  const origin = webOrigin(sender.url);
  return sender.id === extensionId && sender.frameId === 0 && Number.isInteger(sender.tab?.id) && origin !== null && origin === message.origin && webOrigin(sender.tab.url) === origin && typeof message.username === 'string' && message.username.trim() !== '' && byteLength(message.username) <= 512 && typeof message.password === 'string' && byteLength(message.password) > 0 && byteLength(message.password) <= 4096;
}
export function trustedPopup(sender, extensionId) {
  return sender.id === extensionId && !sender.tab && sender.url === `chrome-extension://${extensionId}/popup.html`;
}
