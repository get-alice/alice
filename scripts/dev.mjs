import { spawn, spawnSync } from 'node:child_process';
import { join } from 'node:path';

// macOS Keychain trusts the program that saved a sign-in by its code signature. Debug builds are ad-hoc signed,
// so every rebuild is a new program to Keychain and reading credentials needs approval again. With
// APPLE_SIGNING_IDENTITY set, the daemon is signed with that identity, so one "Always Allow" covers later builds.
const identity = process.platform === 'darwin' ? process.env.APPLE_SIGNING_IDENTITY : undefined;
let cargo;
if (identity) {
  const binary = join(process.env.CARGO_TARGET_DIR || 'target', 'debug', 'alice');
  for (const [command, args] of [['cargo', ['build', '-p', 'alice']], ['codesign', ['--force', '--identifier', 'email.getalice.cli', '--sign', identity, binary]]]) {
    const result = spawnSync(command, args, { stdio: 'inherit' });
    if (result.status !== 0) process.exit(result.status ?? 1);
  }
  cargo = spawn(binary, ['daemon', 'run'], { stdio: 'inherit' });
} else cargo = spawn('cargo', ['run', '-p', 'alice', '--', 'daemon', 'run'], { stdio: 'inherit' });
const vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', '1420'], { stdio: 'inherit' });
function stop() { cargo.kill('SIGINT'); vite.kill('SIGINT'); }
process.on('SIGINT', stop);
process.on('SIGTERM', stop);
vite.on('exit', code => { cargo.kill('SIGINT'); process.exitCode = code || 0; });
