#!/usr/bin/env node
// gyeot-channel: Claude Code가 stdio로 띄우는 channel MCP 서버(contracts §10).

import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { resolveBackoffUnitMs, resolveDaemonUrl, resolveDataDir, tokenPath } from './config.js';
import { DaemonClient } from './daemon.js';
import { Link } from './link.js';
import { describeError, log, routeConsoleToStderr } from './log.js';
import { createServer, pushMessage } from './server.js';

/** 종료 절차 전체의 한도. 이 뒤에는 기다리지 않고 끝낸다. */
const HARD_EXIT_MS = 1000;

async function main(): Promise<void> {
  routeConsoleToStderr();

  const env = process.env;
  const dataDir = resolveDataDir(env);
  const url = resolveDaemonUrl(env);
  if (!url.ok) log('error', url.error);

  const client = new DaemonClient({
    origin: url.ok ? url.origin : undefined,
    configError: url.ok ? undefined : url.error,
    tokenFile: tokenPath(dataDir),
  });

  let server: ReturnType<typeof createServer> | undefined;
  const link = new Link({
    client,
    registration: { pid: process.pid, ppid: process.ppid, cwd: process.cwd() },
    backoffUnitMs: resolveBackoffUnitMs(env),
    onMessage: async (message) => {
      if (server) await pushMessage(server, message);
    },
  });
  server = createServer(client, link);

  let shuttingDown = false;
  const shutdown = (reason: string): void => {
    if (shuttingDown) return;
    shuttingDown = true;
    log('info', `끝냄: ${reason}`);
    setTimeout(() => process.exit(0), HARD_EXIT_MS).unref();
    void (async () => {
      await link.stop();
      await server?.close().catch(() => {});
      process.exit(0);
    })();
  };

  process.stdin.on('end', () => shutdown('stdin 닫힘'));
  process.stdin.on('close', () => shutdown('stdin 닫힘'));
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) process.on(sig, () => shutdown(sig));
  process.stdout.on('error', () => shutdown('stdout 닫힘'));
  process.on('unhandledRejection', (err) => log('error', `처리되지 않은 거부: ${describeError(err)}`));

  server.oninitialized = () => {
    const info = server?.getClientVersion();
    const experimental = Object.keys(server?.getClientCapabilities()?.experimental ?? {});
    log(
      'info',
      `MCP 연결됨: ${info?.name ?? '?'} ${info?.version ?? ''}` +
        (experimental.length ? ` (experimental: ${experimental.join(', ')})` : ''),
    );
    link.start();
  };
  server.onclose = () => shutdown('MCP 연결 닫힘');

  await server.connect(new StdioServerTransport());
}

main().catch((err: unknown) => {
  log('error', `시작 실패: ${describeError(err)}`);
  process.exit(1);
});
