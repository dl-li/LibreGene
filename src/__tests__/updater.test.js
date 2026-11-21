import { describe, expect, it } from 'vitest';
import { compareVersions } from '../hooks/useUpdater';
import {
  createUpdateManifest,
  obsoleteReleasePrefixes,
} from '../../scripts/publish-cloudflare.mjs';

const assets = [
  { name: 'LibreGene.app.tar.gz', signature: 'mac-signature\n' },
  { name: 'LibreGene_1.2.3_x64-setup.exe', signature: 'windows-x64-signature\n' },
  { name: 'LibreGene_1.2.3_arm64-setup.exe', signature: 'windows-arm64-signature\n' },
];

describe('updater versions', () => {
  it('compares stable and prerelease semantic versions', () => {
    expect(compareVersions('0.9.4', '0.9.3')).toBe(1);
    expect(compareVersions('v0.9.4', '0.9.4')).toBe(0);
    expect(compareVersions('1.0.0', '1.0.0-rc.1')).toBe(1);
    expect(compareVersions('1.0.0-beta.2', '1.0.0-beta.11')).toBe(-1);
    expect(compareVersions('invalid', '1.0.0')).toBe(0);
  });
});

describe('Cloudflare update manifest', () => {
  it('maps updater artifacts and embeds their signatures', () => {
    const manifest = createUpdateManifest({
      version: '1.2.3',
      notes: 'Release notes',
      assets,
      pubDate: '2026-11-21T00:00:00.000Z',
    });
    expect(manifest.version).toBe('1.2.3');
    expect(manifest.platforms['darwin-aarch64']).toEqual({
      signature: 'mac-signature',
      url: 'https://libregene.donglin.li/releases/v1.2.3/LibreGene.app.tar.gz',
    });
    expect(manifest.platforms['windows-x86_64'].signature).toBe('windows-x64-signature');
    expect(manifest.platforms['windows-aarch64'].signature).toBe('windows-arm64-signature');
  });

  it('rejects missing signatures and invalid versions', () => {
    expect(() =>
      createUpdateManifest({
        version: '1.2.3',
        notes: '',
        assets: assets.map((asset) => ({ ...asset, signature: '' })),
      }),
    ).toThrow('Missing signature');
    expect(() => createUpdateManifest({ version: '../1.2.3', notes: '', assets })).toThrow(
      'Invalid release version',
    );
  });

  it('selects only obsolete versioned release directories', () => {
    const keys = [
      'latest.json',
      'releases/v1.2.3/LibreGene.app.tar.gz',
      'releases/v1.2.2/LibreGene.app.tar.gz',
      'releases/v1.2.2/LibreGene_x64-setup.exe',
      'releases/v1.2.1/file',
      'releases/manual/file',
    ];
    expect(obsoleteReleasePrefixes(keys, '1.2.3')).toEqual([
      'releases/v1.2.1/',
      'releases/v1.2.2/',
    ]);
  });
});
