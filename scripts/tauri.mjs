import { logError, run } from '@tauri-apps/cli';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const commandIndex = args.findIndex(arg => !arg.startsWith('-'));

if (args[commandIndex] === 'dev') {
  const devConfig = fileURLToPath(new URL('../src-tauri/tauri.dev.conf.json', import.meta.url));
  args.splice(commandIndex + 1, 0, '--config', devConfig);
}

try {
  await run(args, 'pnpm tauri');
} catch (error) {
  logError(error.message);
  process.exitCode = 1;
}
