#!/usr/bin/env python3
"""Prepare an explicit local Tauri signing override; does not sign or install anything."""
import argparse
import datetime
import json
from pathlib import Path
import plistlib
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--identity', required=True, help='Exact valid codesigning certificate name or SHA-1')
    parser.add_argument('--profile', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path, help='New private output directory outside the repository')
    args = parser.parse_args()
    profile_path = args.profile.expanduser().resolve(strict=True)
    result = subprocess.run(['security', 'cms', '-D', '-i', str(profile_path)], capture_output=True, check=True)
    profile = plistlib.loads(result.stdout)
    expiration = profile.get('ExpirationDate')
    if not expiration or expiration.replace(tzinfo=datetime.timezone.utc) <= datetime.datetime.now(datetime.timezone.utc):
        parser.error('Provisioning profile is missing an expiration or has expired')
    entitlements = profile.get('Entitlements', {})
    app_id = entitlements.get('com.apple.application-identifier', '')
    prefixes = profile.get('ApplicationIdentifierPrefix', [])
    if app_id not in [prefix + '.com.autologin.desktop' for prefix in prefixes]:
        parser.error('Profile must explicitly authorize com.autologin.desktop (wildcard profiles are not accepted)')
    groups = entitlements.get('keychain-access-groups', [])
    if not any(app_id == group or (group.endswith('.*') and app_id.startswith(group[:-1])) for group in groups):
        parser.error('Profile does not authorize the application Keychain access group')
    identities = subprocess.run(['security', 'find-identity', '-v', '-p', 'codesigning'], capture_output=True, text=True, check=True).stdout
    if not any(('"' + args.identity + '"') in line or (' ' + args.identity + ' ') in line for line in identities.splitlines()):
        parser.error('Requested signing identity is not a valid local codesigning identity')
    team = entitlements.get('com.apple.developer.team-identifier')
    if not team or team not in profile.get('TeamIdentifier', []):
        parser.error('Provisioning profile has no matching team identifier')
    output = args.output.expanduser().resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    entitlement_path = output / 'LoginDeck.entitlements.plist'
    entitlement_path.write_bytes(plistlib.dumps({
        'com.apple.application-identifier': app_id,
        'com.apple.developer.team-identifier': team,
        'keychain-access-groups': [app_id],
    }))
    config = output / 'tauri.signing.json'
    config.write_text(json.dumps({'bundle': {'macOS': {
        'signingIdentity': args.identity,
        'entitlements': str(entitlement_path),
        'files': {'embedded.provisionprofile': str(profile_path)},
    }}}, indent=2) + '\n')
    print(config)


if __name__ == '__main__':
    main()
