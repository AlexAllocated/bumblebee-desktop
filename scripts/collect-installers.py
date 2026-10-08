"""Collect build products without accidentally publishing intermediate configuration or secrets."""
import hashlib
import pathlib
import shutil
import sys

source, destination = map(pathlib.Path, sys.argv[1:3])
destination.mkdir(parents=True, exist_ok=True)
products = sorted(path for path in source.rglob('*') if path.is_file() and path.suffix.lower() in {'.exe', '.deb', '.appimage'})
if not products:
    raise SystemExit('No installers found')
checksums = []
for path in products:
    target = destination / path.name
    if target.exists():
        raise SystemExit(f'Duplicate installer name: {path.name}')
    shutil.copyfile(path, target)
    checksums.append(f'{hashlib.file_digest(target.open("rb"), "sha256").hexdigest()}  {target.name}')
(destination / 'SHA256SUMS.txt').write_text('\n'.join(checksums) + '\n')
(destination / 'UNSIGNED.txt').write_text('These preview installers are unsigned. Verify SHA256SUMS.txt and download only from the project repository’s official Releases page. Updates are manual; installing a newer version preserves application data.\n')
print(f'Collected {len(products)} installers with SHA-256 checksums.')
