import { cp } from 'node:fs/promises';
import path from 'node:path';

export async function copyNativeHost(targetDir, target, resource, platform = process.platform) {
  const windowsTarget = target ? target.includes('-windows-') : platform === 'win32';
  const hostName = `autologin-native-host${windowsTarget ? '.exe' : ''}`;
  await cp(path.join(targetDir, ...(target ? [target] : []), 'release', hostName), path.join(resource, hostName));
}
