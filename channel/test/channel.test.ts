// 묶은 dist/gyeot-channel.mjs를 실제로 띄워 MCP 클라이언트와 가짜 데몬 사이에서 시험한다.

import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import type { CallToolResult, Notification } from '@modelcontextprotocol/sdk/types.js';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { FakeDaemon, waitFor } from './fake-daemon.js';

const DIST = fileURLToPath(new URL('../dist/gyeot-channel.mjs', import.meta.url));
const IDENT = /^[A-Za-z0-9_]+$/;
/** channel을 띄울 node. 다른 판(예: Node 20)으로 시험하려면 GYEOT_TEST_NODE에 경로를 준다. */
const NODE = process.env['GYEOT_TEST_NODE'] ?? process.execPath;

interface Running {
  client: Client;
  transport: StdioClientTransport;
  notifications: Notification[];
  stderr: () => string;
}

const running: Running[] = [];
let daemon: FakeDaemon;
let home: string;
let token: string;

function newToken(): string {
  return randomBytes(32).toString('hex');
}

async function startChannel(env: Record<string, string> = {}): Promise<Running> {
  const transport = new StdioClientTransport({
    command: NODE,
    args: [DIST],
    env: {
      PATH: process.env['PATH'] ?? '',
      GYEOT_URL: daemon.url,
      GYEOT_HOME: home,
      GYEOT_CHANNEL_BACKOFF_MS: '50',
      ...env,
    },
    stderr: 'pipe',
  });
  let stderr = '';
  transport.stderr?.on('data', (d: Buffer) => (stderr += d.toString('utf8')));
  const client = new Client({ name: 'gyeot-test', version: '0.0.0' }, { capabilities: {} });
  const notifications: Notification[] = [];
  client.fallbackNotificationHandler = async (n) => {
    notifications.push(n);
  };
  await client.connect(transport);
  const r = { client, transport, notifications, stderr: () => stderr };
  running.push(r);
  return r;
}

async function call(r: Running, name: string, args: Record<string, unknown>): Promise<CallToolResult> {
  return (await r.client.callTool({ name, arguments: args })) as CallToolResult;
}

function textOf(res: CallToolResult): string {
  const first = res.content[0];
  return first && first.type === 'text' ? first.text : '';
}

function channelNotes(r: Running): Notification[] {
  return r.notifications.filter((n) => n.method === 'notifications/claude/channel');
}

async function connected(cid = 'ch_1'): Promise<void> {
  await waitFor(() => daemon.streamCount(cid) === 1, 5000, `스트림 ${cid}`);
}

/** 한 번 열었다 닫아 지금 비어 있는 포트를 얻는다. */
async function freePort(): Promise<number> {
  const s = http.createServer();
  await new Promise<void>((r) => s.listen(0, '127.0.0.1', () => r()));
  const port = (s.address() as AddressInfo).port;
  await new Promise<void>((r) => s.close(() => r()));
  return port;
}

beforeAll(() => {
  if (!existsSync(DIST)) throw new Error(`${DIST}가 없습니다. 먼저 npm run build`);
});

beforeEach(async () => {
  home = await mkdtemp(path.join(os.tmpdir(), 'gyeot-channel-'));
  token = newToken();
  await writeFile(path.join(home, 'token'), `${token}\n`);
  daemon = new FakeDaemon(token);
  await daemon.start();
});

afterEach(async () => {
  for (const r of running.splice(0)) await r.client.close().catch(() => {});
  await daemon.stop();
  await rm(home, { recursive: true, force: true });
});

describe('capability와 안내문', () => {
  it('claude/channel과 tools를 선언하고 claude/channel/permission은 선언하지 않는다', async () => {
    const r = await startChannel();
    const caps = r.client.getServerCapabilities();
    expect(caps?.experimental?.['claude/channel']).toEqual({});
    expect(caps?.tools).toEqual({});
    expect(Object.keys(caps?.experimental ?? {})).toEqual(['claude/channel']);
    expect(Object.prototype.hasOwnProperty.call(caps?.experimental ?? {}, 'claude/channel/permission')).toBe(false);
    expect(r.client.getServerVersion()?.name).toBe('gyeot');
  });

  it('안내문이 태그 꼴·reply·chat_id·첫 줄·결정·권한 경계를 알려 준다', async () => {
    const r = await startChannel();
    const text = r.client.getInstructions() ?? '';
    for (const needle of [
      '<channel source="gyeot"',
      'from="dj"',
      'kind="say|choice"',
      'reply',
      'chat_id',
      '첫 줄',
      'request_decision',
      '2~4',
      '[선택]',
      '터미널',
    ]) {
      expect(text).toContain(needle);
    }
  });

  it('도구는 reply와 request_decision 둘이다', async () => {
    const r = await startChannel();
    const { tools } = await r.client.listTools();
    expect(tools.map((t) => t.name).sort()).toEqual(['reply', 'request_decision']);
    const decision = tools.find((t) => t.name === 'request_decision');
    expect(decision?.inputSchema.properties?.['options']).toMatchObject({ minItems: 2, maxItems: 4 });
  });
});

describe('등록과 메시지', () => {
  it('pid·ppid·cwd로 등록하고 토큰을 싣는다', async () => {
    const r = await startChannel();
    await waitFor(() => daemon.of('/v1/channel/register').length === 1);
    const reg = daemon.of('/v1/channel/register')[0]!;
    expect(reg.headers.authorization).toBe(`Bearer ${token}`);
    expect(reg.body).toEqual({ pid: r.transport.pid, ppid: process.pid, cwd: expect.any(String) });
    await connected();
    const stream = daemon.of('/v1/channel/stream')[0]!;
    expect(stream.query.get('cid')).toBe('ch_1');
    expect(stream.query.get('t')).toBe(token);
  });

  it('데몬 message를 notifications/claude/channel로 보낸다(content·meta)', async () => {
    const r = await startChannel();
    await connected();
    daemon.sendMessage('ch_1', { chat_id: 'c_7', text: '빌드 어때?', kind: 'say' });
    await waitFor(() => channelNotes(r).length === 1);
    const note = channelNotes(r)[0]!;
    expect(note.params).toEqual({
      content: '빌드 어때?',
      meta: { chat_id: 'c_7', from: 'dj', via: 'gyeot', kind: 'say' },
    });
    const meta = (note.params as { meta: Record<string, string> }).meta;
    for (const k of Object.keys(meta)) expect(k).toMatch(IDENT);
  });

  it('여러 줄 data·CRLF·조각 나뉜 쓰기·주석·다른 사건·깨진 JSON을 견디고 순서를 지킨다', async () => {
    const r = await startChannel();
    await connected();
    const multi = JSON.stringify({ chat_id: 'c_1', text: '첫째 줄\n둘째 줄', kind: 'say' });
    // data를 두 줄로 나눈다: JSON 문자열 안이 아니라 토큰 사이에서 자른다.
    const cut = multi.indexOf(',"text"');
    daemon.writeRaw('ch_1', `: ping\r\nevent: message\r\ndata: ${multi.slice(0, cut)}\r\ndata: ${multi.slice(cut)}\r\n\r\n`);
    daemon.writeRaw('ch_1', 'event: presence\ndata: {"mood":"idle"}\n\n');
    daemon.writeRaw('ch_1', 'event: message\ndata: {not json\n\n');
    const second = `event: message\ndata: ${JSON.stringify({ chat_id: 'c_2', text: '[선택] 병합 보류', kind: 'choice' })}\n\n`;
    daemon.writeRaw('ch_1', second.slice(0, 9));
    await new Promise((res) => setTimeout(res, 30));
    daemon.writeRaw('ch_1', second.slice(9, 20));
    await new Promise((res) => setTimeout(res, 30));
    daemon.writeRaw('ch_1', second.slice(20));
    await waitFor(() => channelNotes(r).length === 2);
    const [a, b] = channelNotes(r).map((n) => n.params as { content: string; meta: Record<string, string> });
    expect(a?.content).toBe('첫째 줄\n둘째 줄');
    expect(a?.meta['chat_id']).toBe('c_1');
    expect(b?.content).toBe('[선택] 병합 보류');
    expect(b?.meta).toEqual({ chat_id: 'c_2', from: 'dj', via: 'gyeot', kind: 'choice' });
    expect(r.stderr()).toContain('JSON이 아니어서 버림');
  });

  it('chat_id가 계약 밖(따옴표·없음)이면 알림을 보내지 않는다', async () => {
    const r = await startChannel();
    await connected();
    daemon.sendMessage('ch_1', { chat_id: 'a"b', text: '주입?', kind: 'say' });
    daemon.sendMessage('ch_1', { text: 'chat_id 없음', kind: 'say' });
    daemon.sendMessage('ch_1', { chat_id: 'ok_1', text: '정상', kind: 'say' });
    await waitFor(() => channelNotes(r).length === 1);
    await new Promise((res) => setTimeout(res, 100));
    expect(channelNotes(r).map((n) => (n.params as { content: string }).content)).toEqual(['정상']);
  });
});

describe('도구', () => {
  it('reply는 cid·chat_id·text를 데몬에 보낸다', async () => {
    const r = await startChannel();
    await connected();
    const res = await call(r, 'reply', { chat_id: 'c_7', text: '빌드 통과했어요.\n자세히: 테스트 42개' });
    expect(res.isError).toBeFalsy();
    const sent = daemon.of('/v1/channel/reply');
    expect(sent).toHaveLength(1);
    expect(sent[0]!.body).toEqual({ cid: 'ch_1', chat_id: 'c_7', text: '빌드 통과했어요.\n자세히: 테스트 42개' });
    expect(sent[0]!.headers.authorization).toBe(`Bearer ${token}`);
  });

  it('request_decision은 질문·선택지를 보내고 decision_id를 알려 준다', async () => {
    const r = await startChannel();
    await connected();
    const res = await call(r, 'request_decision', { question: 'PR #3 병합할까요?', options: [' 병합 ', '보류'] });
    expect(res.isError).toBeFalsy();
    expect(textOf(res)).toContain('d_1');
    expect(daemon.of('/v1/channel/decision')[0]!.body).toEqual({
      cid: 'ch_1',
      question: 'PR #3 병합할까요?',
      options: ['병합', '보류'],
    });
  });

  it('request_decision 선택지가 1개·5개·중복·빈 것이면 isError이고 데몬에 보내지 않는다', async () => {
    const r = await startChannel();
    await connected();
    const cases: unknown[] = [['하나'], ['1', '2', '3', '4', '5'], ['같음', '같음'], ['가', ' '], 'abc', undefined];
    for (const options of cases) {
      const res = await call(r, 'request_decision', { question: '고를까요?', options });
      expect(res.isError, JSON.stringify(options)).toBe(true);
      expect(textOf(res)).toMatch(/^입력이 올바르지 않음: /);
    }
    expect(textOf(await call(r, 'request_decision', { question: '고를까요?', options: ['하나'] }))).toContain('2~4개');
    const empty = await call(r, 'request_decision', { question: '  ', options: ['a', 'b'] });
    expect(empty.isError).toBe(true);
    expect(daemon.of('/v1/channel/decision')).toHaveLength(0);
  });

  it('reply 입력 검사: chat_id 없음·따옴표·빈 text는 isError', async () => {
    const r = await startChannel();
    await connected();
    for (const args of [{ text: 'x' }, { chat_id: 'a"b', text: 'x' }, { chat_id: 'c', text: '   ' }, { chat_id: 'c' }]) {
      const res = await call(r, 'reply', args);
      expect(res.isError, JSON.stringify(args)).toBe(true);
    }
    expect(daemon.of('/v1/channel/reply')).toHaveLength(0);
  });

  it('모르는 도구는 프로토콜 오류다', async () => {
    const r = await startChannel();
    await expect(call(r, 'approve', {})).rejects.toThrow(/없는 도구/);
  });
});

describe('데몬이 없거나 설정이 틀렸을 때', () => {
  it('데몬이 꺼져 있으면 도구는 isError "곁 데몬에 닿지 않음: …"', async () => {
    const port = await freePort();
    const r = await startChannel({ GYEOT_URL: `http://127.0.0.1:${port}` });
    for (const [name, args] of [
      ['reply', { chat_id: 'c', text: 'x' }],
      ['request_decision', { question: 'q', options: ['a', 'b'] }],
    ] as const) {
      const res = await call(r, name, args);
      expect(res.isError).toBe(true);
      expect(textOf(res)).toMatch(/^곁 데몬에 닿지 않음: /);
    }
  });

  it('토큰 파일이 없으면 isError로 알려 준다', async () => {
    await rm(path.join(home, 'token'));
    const r = await startChannel();
    const res = await call(r, 'reply', { chat_id: 'c', text: 'x' });
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/^곁 데몬에 닿지 않음: 토큰 파일이 없습니다/);
    expect(daemon.requests).toHaveLength(0);
  });

  it('GYEOT_URL이 루프백이 아니면 토큰을 보내지 않고 isError', async () => {
    const r = await startChannel({ GYEOT_URL: 'http://192.0.2.1:47321' });
    const res = await call(r, 'reply', { chat_id: 'c', text: 'x' });
    expect(res.isError).toBe(true);
    expect(textOf(res)).toContain('GYEOT_URL');
  });

  it('토큰이 틀리면 isError이고, stderr·오류 문구에 토큰이 나오지 않는다', async () => {
    const wrong = newToken();
    await writeFile(path.join(home, 'token'), wrong);
    const r = await startChannel();
    const res = await call(r, 'reply', { chat_id: 'c', text: 'x' });
    expect(res.isError).toBe(true);
    expect(textOf(res)).toContain('토큰');
    await waitFor(() => daemon.of('/v1/channel/register').length >= 2);
    expect(textOf(res)).not.toContain(wrong);
    expect(r.stderr()).not.toContain(wrong);
    expect(r.stderr()).not.toContain(token);
  });

  it('데몬이 늦게 떠도 다시 시도해서 등록한다', async () => {
    const port = await freePort();
    const r = await startChannel({ GYEOT_URL: `http://127.0.0.1:${port}` });
    await new Promise((res) => setTimeout(res, 200));
    const late = new FakeDaemon(token);
    await late.start(port);
    try {
      await waitFor(() => late.streamCount('ch_1') === 1, 8000, '늦은 데몬 스트림');
      late.sendMessage('ch_1', { chat_id: 'c', text: '늦게 왔어요', kind: 'say' });
      await waitFor(() => channelNotes(r).length === 1);
    } finally {
      await r.client.close();
      await late.stop();
    }
  });
});

describe('재연결', () => {
  it.each([
    ['연결이 끊기면', false],
    ['응답이 정상으로 끝나면', true],
  ])('데몬 스트림이 %s 같은 cid로 다시 연결한다', async (_label, graceful) => {
    const r = await startChannel();
    await connected();
    for (let round = 2; round <= 3; round++) {
      daemon.dropStreams(graceful);
      await waitFor(
        () => daemon.of('/v1/channel/stream').length >= round && daemon.streamCount('ch_1') === 1,
        5000,
        `재연결 ${round}`,
      );
    }
    expect(daemon.of('/v1/channel/register')).toHaveLength(1);
    daemon.sendMessage('ch_1', { chat_id: 'c_9', text: '다시 붙었나요?', kind: 'say' });
    await waitFor(() => channelNotes(r).length === 1);
    expect(r.stderr()).toContain('cid=ch_1');
    expect(r.stderr()).not.toContain(token);
  });

  it('데몬이 cid를 잊으면(재시작) 다시 등록하고 새 cid로 받는다', async () => {
    const r = await startChannel();
    await connected();
    daemon.forgetAll();
    await connected('ch_2');
    expect(daemon.of('/v1/channel/register')).toHaveLength(2);
    daemon.sendMessage('ch_2', { chat_id: 'c_1', text: '새 cid', kind: 'say' });
    await waitFor(() => channelNotes(r).length === 1);
    const res = await call(r, 'reply', { chat_id: 'c_1', text: '받았어요' });
    expect(res.isError).toBeFalsy();
    expect(daemon.of('/v1/channel/reply').at(-1)?.body).toMatchObject({ cid: 'ch_2' });
  });

  it('도구 호출에서 데몬이 cid를 모른다고 하면 다시 등록하고 한 번 더 보낸다', async () => {
    const r = await startChannel();
    await connected();
    daemon.forgetCids();
    const res = await call(r, 'reply', { chat_id: 'c_1', text: '다시' });
    expect(res.isError).toBeFalsy();
    expect(daemon.of('/v1/channel/reply').map((q) => (q.body as { cid: string }).cid)).toEqual(['ch_1', 'ch_2']);
    await connected('ch_2');
  });
});

describe('종료', () => {
  it('stdin이 닫히면(신호 없이) 곧바로 unregister를 보내고 exit 0으로 끝난다', async () => {
    const child = rawChild();
    try {
      child.init();
      await connected();
      const exited = new Promise<number | null>((res) => child.proc.once('exit', (code) => res(code)));
      const t0 = Date.now();
      child.proc.stdin.end();
      // Windows의 Claude Code는 stdin을 닫고 300ms 뒤 프로세스 트리를 끝낸다. 넉넉히 1초 안.
      await waitFor(() => daemon.of('/v1/channel/unregister').length === 1, 1000, 'unregister');
      expect(daemon.of('/v1/channel/unregister')[0]!.body).toEqual({ cid: 'ch_1' });
      expect(await exited).toBe(0);
      expect(Date.now() - t0).toBeLessThan(1500);
    } finally {
      child.proc.kill('SIGKILL');
    }
  });

  it.skipIf(process.platform === 'win32')('SIGTERM을 받으면 unregister를 보내고 끝난다', async () => {
    const r = await startChannel();
    await connected();
    const pid = r.transport.pid!;
    process.kill(pid, 'SIGTERM');
    await waitFor(() => daemon.of('/v1/channel/unregister').length === 1);
    await waitFor(() => !alive(pid), 3000, '프로세스 끝남');
  });
});

describe('stdout 순수성·프로토콜 판', () => {
  it('stdout에는 JSON-RPC만 나오고, 2026-07-28을 청해도 그 판으로 협상하지 않는다', async () => {
    const child = rawChild();
    try {
      child.init('2026-07-28');
      await connected();
      daemon.sendMessage('ch_1', { chat_id: 'c', text: '안녕', kind: 'say' });
      child.send({ jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'reply', arguments: { chat_id: 'c', text: 'x' } } });
      await waitFor(() => child.out().includes('"id":2') && child.out().includes('notifications/claude/channel'));
      const lines = child.out().split('\n').filter((l) => l !== '');
      const msgs = lines.map((l) => JSON.parse(l) as { jsonrpc: string; id?: number; result?: { protocolVersion?: string } });
      for (const m of msgs) expect(m.jsonrpc).toBe('2.0');
      const init = msgs.find((m) => m.id === 1);
      expect(init?.result?.protocolVersion).toBeDefined();
      expect(init?.result?.protocolVersion).not.toBe('2026-07-28');
    } finally {
      child.proc.kill('SIGKILL');
    }
  });
});

/** SDK 클라이언트 없이 줄 단위 JSON-RPC로 직접 말하는 channel 프로세스. */
function rawChild() {
  const proc = spawn(NODE, [DIST], {
    env: { PATH: process.env['PATH'] ?? '', GYEOT_URL: daemon.url, GYEOT_HOME: home, GYEOT_CHANNEL_BACKOFF_MS: '50' },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  let out = '';
  proc.stdout.on('data', (d: Buffer) => (out += d.toString('utf8')));
  proc.stderr.resume();
  const send = (m: unknown): void => void proc.stdin.write(`${JSON.stringify(m)}\n`);
  return {
    proc,
    send,
    out: () => out,
    /** initialize → (응답을 받은 뒤) initialized */
    init(protocolVersion = '2025-11-25'): void {
      send({
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: { protocolVersion, capabilities: {}, clientInfo: { name: 'raw', version: '0' } },
      });
      void waitFor(() => out.includes('"id":1')).then(() => send({ jsonrpc: '2.0', method: 'notifications/initialized' }));
    },
  };
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
