import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const cargo = process.platform === 'win32' ? 'cargo.exe' : 'cargo';
const shell = process.platform === 'win32';

const steps = [
  ['Frontend formatting', npm, ['run', 'format:check'], root],
  ['Frontend lint', npm, ['run', 'lint'], root],
  ['Frontend type check', npm, ['run', 'typecheck'], root],
  ['Frontend tests', npm, ['test'], root],
  ['Layout performance budgets', npm, ['run', 'benchmark:layout'], root],
  ['Frontend build', npm, ['run', 'build'], root],
  ['Core check', cargo, ['check', '-p', 'libregene-core', '--locked'], `${root}backend`],
  ['Core tests', cargo, ['test', '-p', 'libregene-core', '--tests', '--locked'], `${root}backend`],
  [
    'Core Clippy',
    cargo,
    ['clippy', '-p', 'libregene-core', '--lib', '--locked', '--', '-D', 'warnings'],
    `${root}backend`,
  ],
  ['Tauri tests', cargo, ['test', '--lib', '--locked'], `${root}src-tauri`],
  [
    'Tauri Clippy',
    cargo,
    ['clippy', '--lib', '--locked', '--', '-D', 'warnings'],
    `${root}src-tauri`,
  ],
  ['Tauri build', cargo, ['build', '--locked'], `${root}src-tauri`],
];

for (const [name, command, args, cwd] of steps) {
  console.log(`\n==> ${name}`);
  const result = spawnSync(command, args, { cwd, stdio: 'inherit', shell });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}
