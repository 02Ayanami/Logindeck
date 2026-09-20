#!/usr/bin/env python3
"""Install only LoginDeck's per-user Edge Native Messaging registration."""
import argparse
import json
from pathlib import Path
import shutil
import os

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', required=True, type=Path)
parser.add_argument('--uninstall', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[3]
identity = json.loads((root / 'browser-extension/identity.json').read_text())
name = 'com.autologin.native'
host_dir = Path.home() / 'Library/Application Support/Microsoft Edge/NativeMessagingHosts'
install_dir = Path.home() / 'Library/Application Support/LoginDeck/native-host'
manifest_path = host_dir / (name + '.json')
binary_path = install_dir / 'autologin-native-host'
manifest = {'name':name,'description':'LoginDeck Edge login capture','path':str(binary_path),'type':'stdio','allowed_origins':['chrome-extension://' + identity['id'] + '/']}
if manifest_path.exists() and json.loads(manifest_path.read_text()) != manifest:
    parser.error('An unrelated or different registration already exists; review it before replacing')
if args.uninstall:
    manifest_path.unlink(missing_ok=True)
    binary_path.unlink(missing_ok=True)
    print('Removed LoginDeck Edge registration and host executable; vault data retained.')
else:
    source=args.binary.expanduser().resolve(strict=True)
    if not source.is_file() or not os.access(source,os.X_OK): parser.error('Binary must be an executable file')
    install_dir.mkdir(parents=True,exist_ok=True,mode=0o700)
    host_dir.mkdir(parents=True,exist_ok=True)
    temporary=install_dir / 'autologin-native-host.new'
    shutil.copyfile(source,temporary)
    temporary.chmod(0o700)
    temporary.replace(binary_path)
    manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
    manifest_path.chmod(0o600)
    print('Installed:', manifest_path)
