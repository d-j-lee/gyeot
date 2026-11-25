// MCP 서버: channel capability·안내문·도구(reply, request_decision).

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import {
  CallToolRequestSchema,
  ErrorCode,
  ListToolsRequestSchema,
  McpError,
  type CallToolResult,
  type Tool,
} from '@modelcontextprotocol/sdk/types.js';
import { DaemonError, type DaemonClient } from './daemon.js';
import { INSTRUCTIONS } from './instructions.js';
import type { Link } from './link.js';
import { describeError, log } from './log.js';
import { LIMITS, checkDecision, checkReply, sanitizeMeta, type ChannelMessage } from './validate.js';

export const SERVER_NAME = 'gyeot';
export const SERVER_VERSION = '0.1.0';
export const CHANNEL_METHOD = 'notifications/claude/channel';

const TOOL_TIMEOUT_MS = 5000;

/**
 * capability. `claude/channel/permission`은 선언하지 않는다(키 자체를 두지 않는다):
 * 권한 승인은 터미널에서 사람이 한다(G-005). `false`도 v2.1.234 이전 Claude Code는 선언으로 봤다.
 */
export const CAPABILITIES = {
  experimental: { 'claude/channel': {} },
  tools: {},
} as const;

export const TOOLS: Tool[] = [
  {
    name: 'reply',
    title: '곁으로 답하기',
    description:
      '곁(아바타)을 통해 dj에게 답을 보냅니다. <channel source="gyeot">로 받은 chat_id를 그대로 넘기세요. ' +
      '첫 줄은 소리 내어 읽히니 한 문장 요약으로 쓰고, 자세한 내용은 둘째 줄부터 쓰세요.',
    inputSchema: {
      type: 'object',
      properties: {
        chat_id: { type: 'string', minLength: 1, maxLength: LIMITS.chatId, description: '받은 <channel> 태그의 chat_id' },
        text: {
          type: 'string',
          minLength: 1,
          maxLength: LIMITS.text,
          description: '보낼 글. 첫 줄 = 읽어 줄 한 문장 요약, 둘째 줄부터 = 자세한 내용',
        },
      },
      required: ['chat_id', 'text'],
      additionalProperties: false,
    },
    annotations: { destructiveHint: false, openWorldHint: false },
  },
  {
    name: 'request_decision',
    title: '곁에 결정 묻기',
    description:
      'dj가 정해야 할 일을 곁에 선택지로 띄웁니다. 선택지는 2~4개의 짧은 말로 주세요. ' +
      'dj가 고르면 kind="choice"인 "[선택] <선택지>" 메시지가 이 세션으로 돌아옵니다. 권한 승인에는 쓰지 마세요.',
    inputSchema: {
      type: 'object',
      properties: {
        question: { type: 'string', minLength: 1, maxLength: LIMITS.question, description: '물을 내용(한두 문장)' },
        options: {
          type: 'array',
          items: { type: 'string', minLength: 1, maxLength: LIMITS.option },
          minItems: LIMITS.minOptions,
          maxItems: LIMITS.maxOptions,
          uniqueItems: true,
          description: '선택지 2~4개(짧은 말)',
        },
      },
      required: ['question', 'options'],
      additionalProperties: false,
    },
    annotations: { destructiveHint: false, openWorldHint: false },
  },
];

export function createServer(client: DaemonClient, link: Link): Server {
  const server = new Server(
    { name: SERVER_NAME, version: SERVER_VERSION },
    { capabilities: CAPABILITIES, instructions: INSTRUCTIONS },
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: TOOLS }));

  server.setRequestHandler(CallToolRequestSchema, async (req): Promise<CallToolResult> => {
    const args = req.params.arguments ?? {};
    switch (req.params.name) {
      case 'reply': {
        const input = checkReply(args);
        if (!input.ok) return toolError(`입력이 올바르지 않음: ${input.error}`);
        return withCid(client, link, async (cid) => {
          await client.post('/v1/channel/reply', { cid, ...input.value }, { timeoutMs: TOOL_TIMEOUT_MS, cidBound: true });
          return ok('곁으로 보냈습니다.');
        });
      }
      case 'request_decision': {
        const input = checkDecision(args);
        if (!input.ok) return toolError(`입력이 올바르지 않음: ${input.error}`);
        return withCid(client, link, async (cid) => {
          const res = await client.post(
            '/v1/channel/decision',
            { cid, ...input.value },
            { timeoutMs: TOOL_TIMEOUT_MS, cidBound: true },
          );
          const id = (res as { decision_id?: unknown }).decision_id;
          if (typeof id !== 'string' || id === '') throw new DaemonError('rejected', '데몬 응답에 decision_id가 없습니다');
          return ok(
            `곁에 결정 요청을 띄웠습니다(decision_id=${id}). dj가 고르면 "[선택] …" 메시지로 돌아옵니다. 기다리는 동안 다른 일을 해도 됩니다.`,
          );
        });
      }
      default:
        throw new McpError(ErrorCode.InvalidParams, `없는 도구: ${req.params.name}`);
    }
  });

  return server;
}

/** 데몬 메시지를 Claude Code channel 알림으로 보낸다. */
export async function pushMessage(server: Server, message: ChannelMessage): Promise<void> {
  await server.notification({
    method: CHANNEL_METHOD,
    params: {
      content: message.text,
      meta: sanitizeMeta({ chat_id: message.chat_id, from: 'dj', via: 'gyeot', kind: message.kind }),
    },
  });
}

/** cid가 필요한 호출. 데몬이 cid를 모르면 다시 등록해서, 토큰이 바뀌었으면 다시 읽어서 한 번씩 더 해 본다. */
async function withCid(
  client: DaemonClient,
  link: Link,
  call: (cid: string) => Promise<CallToolResult>,
): Promise<CallToolResult> {
  let retriedCid = false;
  let retriedAuth = false;
  for (;;) {
    let cid: string | undefined;
    try {
      cid = await link.ensureCid();
      return await call(cid);
    } catch (err) {
      if (err instanceof DaemonError && err.kind === 'unknown_cid' && cid !== undefined && !retriedCid) {
        retriedCid = true;
        link.invalidate(cid);
        continue;
      }
      if (err instanceof DaemonError && err.kind === 'unauthorized' && !retriedAuth) {
        retriedAuth = true; // DaemonClient가 토큰을 비워 두었다. 파일을 다시 읽어 한 번 더.
        continue;
      }
      return daemonToolError(err, client);
    }
  }
}

function daemonToolError(err: unknown, client: DaemonClient): CallToolResult {
  if (err instanceof DaemonError) {
    switch (err.kind) {
      case 'unreachable':
      case 'not_daemon':
        // 확인에 실패한 서버는 데몬이 없는 것과 같이 다룬다(§13.1). 토큰도 페이로드도 보내지 않았다.
        return toolError(`곁 데몬에 닿지 않음: ${err.message}`);
      case 'unauthorized':
        return toolError(`곁 데몬이 토큰을 받지 않음: 토큰 파일(${client.tokenFile})이 데몬의 것과 같은지 확인하세요`);
      case 'unknown_cid':
      case 'rejected':
        return toolError(`곁 데몬이 요청을 거절함: ${err.message}`);
    }
  }
  log('error', `도구 처리 중 오류: ${describeError(err)}`);
  return toolError(`곁 channel 내부 오류: ${describeError(err)}`);
}

function ok(text: string): CallToolResult {
  return { content: [{ type: 'text', text }] };
}

function toolError(text: string): CallToolResult {
  return { content: [{ type: 'text', text }], isError: true };
}
