import { spawn } from 'node:child_process';

export const runChildWithCleanup = async ({ args, cwd, env, stop }) => {
  const child = spawn(process.execPath, args, {
    cwd,
    env,
    stdio: 'inherit',
  });
  let stopPromise;
  let interrupted = false;

  const stopOnce = () => {
    stopPromise ??= Promise.resolve(stop());
    return stopPromise;
  };
  const signalHandlers = new Map(
    ['SIGINT', 'SIGTERM'].map((signal) => [
      signal,
      () => {
        interrupted = true;
        child.kill(signal);
        void stopOnce();
      },
    ]),
  );

  for (const [signal, handler] of signalHandlers) process.once(signal, handler);

  try {
    const exitCode = await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code) => resolve(code ?? 1));
    });
    return interrupted ? 130 : exitCode;
  } finally {
    for (const [signal, handler] of signalHandlers) process.off(signal, handler);
    await stopOnce();
  }
};
