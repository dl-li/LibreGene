import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const BASE_URL = 'https://libregene.donglin.li';
const BUCKET = 'libregene-releases';
const VERSION_PATTERN = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

export function createUpdateManifest({ version, notes, assets, baseUrl = BASE_URL, pubDate }) {
  if (!VERSION_PATTERN.test(version)) throw new Error(`Invalid release version: ${version}`);
  const byName = new Map(assets.map((asset) => [asset.name, asset]));
  const targets = [
    ['darwin-aarch64', (name) => name.endsWith('.app.tar.gz')],
    ['windows-x86_64', (name) => /(?:^|_)x64-setup\.exe$/.test(name)],
    ['windows-aarch64', (name) => /(?:^|_)arm64-setup\.exe$/.test(name)],
  ];
  const platforms = {};
  for (const [target, matches] of targets) {
    const names = [...byName.keys()].filter(matches);
    if (names.length !== 1) {
      throw new Error(`Expected one updater asset for ${target}, found ${names.length}`);
    }
    const name = names[0];
    const signature = byName.get(name)?.signature?.trim();
    if (!signature) throw new Error(`Missing signature for ${name}`);
    platforms[target] = {
      signature,
      url: `${baseUrl}/releases/v${version}/${encodeURIComponent(name)}`,
    };
  }
  return {
    version,
    notes,
    pub_date: pubDate || new Date().toISOString(),
    platforms,
  };
}

export function obsoleteReleasePrefixes(keys, currentVersion) {
  const prefixes = new Set();
  for (const key of keys) {
    const match = key.match(
      /^releases\/(v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\//,
    );
    if (match && match[1] !== `v${currentVersion}`) prefixes.add(`releases/${match[1]}/`);
  }
  return [...prefixes].sort();
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: options.capture ? ['ignore', 'pipe', 'inherit'] : 'inherit',
    env: { ...process.env, AWS_EC2_METADATA_DISABLED: 'true' },
    encoding: 'utf8',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited with status ${result.status}`);
  return result.stdout;
}

async function verifyPublishedManifest(expected) {
  const response = await fetch(`${BASE_URL}/latest.json`, { cache: 'no-store' });
  if (!response.ok) throw new Error(`Published manifest returned HTTP ${response.status}`);
  const actual = await response.json();
  if (actual.version !== expected.version) {
    throw new Error(`Published manifest is ${actual.version}, expected ${expected.version}`);
  }
  for (const [target, platform] of Object.entries(expected.platforms)) {
    if (actual.platforms?.[target]?.signature !== platform.signature) {
      throw new Error(`Published signature does not match for ${target}`);
    }
    const download = await fetch(platform.url, { method: 'HEAD' });
    if (!download.ok) throw new Error(`${target} download returned HTTP ${download.status}`);
  }
}

function listReleaseKeys(endpoint) {
  const keys = [];
  let continuation;
  do {
    const args = [
      's3api',
      'list-objects-v2',
      '--bucket',
      BUCKET,
      '--prefix',
      'releases/',
      '--endpoint-url',
      endpoint,
      '--output',
      'json',
    ];
    if (continuation) args.push('--continuation-token', continuation);
    const page = JSON.parse(run('aws', args, { capture: true }));
    keys.push(...(page.Contents || []).map((item) => item.Key));
    continuation = page.IsTruncated ? page.NextContinuationToken : null;
  } while (continuation);
  return keys;
}

async function publish(version, assetsDirectory, notesPath) {
  const accountId = process.env.CLOUDFLARE_ACCOUNT_ID;
  if (!accountId) throw new Error('CLOUDFLARE_ACCOUNT_ID is required');
  if (!process.env.CLOUDFLARE_R2_ACCESS_KEY_ID || !process.env.CLOUDFLARE_R2_SECRET_ACCESS_KEY) {
    throw new Error('Cloudflare R2 credentials are required');
  }
  process.env.AWS_ACCESS_KEY_ID = process.env.CLOUDFLARE_R2_ACCESS_KEY_ID;
  process.env.AWS_SECRET_ACCESS_KEY = process.env.CLOUDFLARE_R2_SECRET_ACCESS_KEY;
  process.env.AWS_DEFAULT_REGION = 'auto';

  const directory = resolve(assetsDirectory);
  const names = readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name !== 'latest.json')
    .map((entry) => entry.name);
  const assets = names.map((name) => {
    const signaturePath = join(directory, `${name}.sig`);
    return {
      name,
      signature: existsSync(signaturePath) ? readFileSync(signaturePath, 'utf8') : null,
    };
  });
  const notes = existsSync(notesPath) ? readFileSync(notesPath, 'utf8') : '';
  const manifest = createUpdateManifest({ version, notes, assets });
  const manifestPath = join(directory, 'latest.json');
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);

  const endpoint = `https://${accountId}.r2.cloudflarestorage.com`;
  const releasePrefix = `releases/v${version}/`;
  run('aws', [
    's3',
    'sync',
    directory,
    `s3://${BUCKET}/${releasePrefix}`,
    '--exclude',
    'latest.json',
    '--cache-control',
    'public,max-age=31536000,immutable',
    '--only-show-errors',
    '--endpoint-url',
    endpoint,
  ]);
  run('aws', [
    's3',
    'cp',
    manifestPath,
    `s3://${BUCKET}/latest.json`,
    '--content-type',
    'application/json',
    '--cache-control',
    'no-cache',
    '--only-show-errors',
    '--endpoint-url',
    endpoint,
  ]);

  await verifyPublishedManifest(manifest);

  const obsolete = obsoleteReleasePrefixes(listReleaseKeys(endpoint), version);
  for (const prefix of obsolete) {
    run('aws', [
      's3',
      'rm',
      `s3://${BUCKET}/${prefix}`,
      '--recursive',
      '--only-show-errors',
      '--endpoint-url',
      endpoint,
    ]);
  }
  console.log(`Published LibreGene v${version}; removed ${obsolete.length} old release(s).`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [version, assetsDirectory, notesPath] = process.argv.slice(2);
  if (!version || !assetsDirectory || !notesPath) {
    console.error('Usage: node scripts/publish-cloudflare.mjs <version> <assets-dir> <notes-path>');
    process.exit(1);
  }
  await publish(version, assetsDirectory, notesPath);
}
