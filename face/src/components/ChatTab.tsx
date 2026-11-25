import { useEffect, useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { Session } from '../types.ts';
import type { ChatMessage } from '../state/reducer.ts';
import { agentLabel } from '../lib/labels.ts';
import { clockKo } from '../lib/time.ts';
import { IconCopy, IconInfo, IconMic, IconSend, IconWarn } from './Icons.tsx';

export const CHANNEL_COMMAND = 'claude --dangerously-load-development-channels server:gyeot';
export const MAX_SAY_CHARS = 4000;

export interface ChatTabProps {
  sessions: Session[];
  messages: ChatMessage[];
  target: string;
  onTarget: (sessionId: string) => void;
  onSend: (text: string) => void;
  onResend: (id: string) => void;
  onListening: (on: boolean) => void;
  autoFocus?: boolean;
  onAutoFocused?: () => void;
}

/** "자동"일 때 계약(§7)대로 고를 세션: channel 이 있는 세션 중 가장 최근에 움직인 것. */
export function autoTarget(sessions: Session[]): Session | null {
  return sessions.filter((s) => s.channel).sort((a, b) => b.last_ms - a.last_ms)[0] ?? null;
}

/** 엔터로 보낼지: 한글 조합 중(IME)이거나 Shift 를 누르면 보내지 않는다. */
export function shouldSend(e: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'isComposing' | 'keyCode' | 'altKey' | 'ctrlKey' | 'metaKey'>): boolean {
  if (e.key !== 'Enter') return false;
  if (e.shiftKey || e.altKey) return false;
  if (e.isComposing || e.keyCode === 229) return false;
  return true;
}

/** 이유 문구 안의 명령 조각(--옵션, server:이름)을 코드로 보인다. */
function ReasonText({ text }: { text: string }) {
  const parts = text.split(/(--[a-z][a-z0-9-]*(?:\s+server:\S+)?)/g);
  return (
    <>
      {parts.map((p, i) => (i % 2 === 1 ? <code key={i}>{p}</code> : <span key={i}>{p}</span>))}
    </>
  );
}

function CopyCommand() {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(CHANNEL_COMMAND);
      setCopied(true);
      setTimeout(() => setCopied(false), 2_000);
    } catch {
      setCopied(false);
    }
  };
  return (
    <button type="button" class="btn btn--xs" onClick={() => void copy()}>
      <IconCopy size={12} />
      {copied ? '복사했어요' : '시작 명령 복사'}
    </button>
  );
}

function MessageView({ m, onResend }: { m: ChatMessage; onResend: (id: string) => void }) {
  if (m.role === 'claude') {
    return (
      <li class="msg msg--claude">
        <div class="msg__head">
          <span class="msg__who">Claude</span>
          {m.project && <span class="msg__where">{m.project}</span>}
          <time class="msg__time">{clockKo(m.at_ms)}</time>
        </div>
        <div class="msg__text">{m.text}</div>
      </li>
    );
  }
  return (
    <li class="msg msg--me">
      <div class="msg__text">{m.text}</div>
      <div class="msg__meta">
        <time class="msg__time">{clockKo(m.at_ms)}</time>
        {m.status === 'sending' && <span>보내는 중…</span>}
        {m.status === 'sent' && (
          <span title="channel에 넘겼어요. Claude가 읽었다는 뜻은 아니에요">
            {m.project ? `${m.project} 세션에 전달 요청함` : '세션에 전달 요청함'}
            {m.answered && ' · 답 받음'}
          </span>
        )}
      </div>
      {m.status === 'sent' && m.noReply && !m.answered && (
        <div class="inline-hint" role="status">
          <IconInfo size={14} />
          <div>
            <p class="inline-hint__text">
              답이 없으면 그 세션을 <code>{CHANNEL_COMMAND}</code> 로 시작했는지 확인하세요.
            </p>
            <div class="inline-hint__actions">
              <CopyCommand />
            </div>
          </div>
        </div>
      )}
      {(m.status === 'undelivered' || m.status === 'error') && (
        <div class="inline-hint inline-hint--warn" role="status">
          <IconWarn size={14} />
          <div>
            <p class="inline-hint__title">{m.status === 'undelivered' ? '세션에 전달하지 못했어요' : '보내지 못했어요'}</p>
            {m.reason && (
              <p class="inline-hint__text">
                <ReasonText text={m.reason} />
              </p>
            )}
            <div class="inline-hint__actions">
              {m.reason?.includes('--dangerously-load-development-channels') && <CopyCommand />}
              <button type="button" class="btn btn--xs" onClick={() => onResend(m.id)}>
                다시 보내기
              </button>
            </div>
          </div>
        </div>
      )}
    </li>
  );
}

export function ChatTab({ sessions, messages, target, onTarget, onSend, onResend, onListening, autoFocus, onAutoFocused }: ChatTabProps) {
  const [text, setText] = useState('');
  const listRef = useRef<HTMLOListElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const stickRef = useRef(true);

  const selected = target ? sessions.find((s) => s.id === target) ?? null : null;
  const auto = autoTarget(sessions);
  const effective = selected ?? auto;
  const connected = Boolean(effective?.channel);

  useLayoutEffect(() => {
    const el = listRef.current;
    if (el && stickRef.current) el.scrollTop = el.scrollHeight;
  }, [messages.length, messages[messages.length - 1]?.status]);

  useEffect(() => {
    if (!autoFocus) return;
    inputRef.current?.focus();
    onAutoFocused?.();
  }, [autoFocus]);

  useEffect(() => () => onListening(false), []);

  const send = () => {
    // 상태보다 입력창의 지금 값을 믿는다(받아쓰기 등이 input 이벤트 없이 넣는 경우).
    const t = (inputRef.current?.value ?? text).trim();
    if (!t) return;
    onSend(t);
    setText('');
    stickRef.current = true;
  };

  return (
    <div class="chat">
      <div class="chat__target">
        <label for="chat-target" class="chat__label">
          보낼 곳
        </label>
        <select
          id="chat-target"
          class="select"
          value={target}
          onChange={(e) => onTarget((e.currentTarget as HTMLSelectElement).value)}
        >
          <option value="">{auto ? `자동 · ${auto.project}` : '자동 (연결된 세션 없음)'}</option>
          {sessions.map((s) => (
            <option key={s.id} value={s.id}>
              {s.project || s.id} · {agentLabel(s.agent)}
              {s.channel ? ' · 연결됨' : ''}
            </option>
          ))}
        </select>
        <span class={`pill ${connected ? 'pill--ok' : 'pill--muted'}`} title={connected ? 'channel로 이어져 있어요' : 'channel이 없어 말을 전할 수 없어요'}>
          {connected ? '연결됨' : '연결 안 됨'}
        </span>
      </div>

      <ol
        class="chat__list"
        ref={listRef}
        aria-label="대화 기록"
        aria-live="off"
        onScroll={(e) => {
          const el = e.currentTarget as HTMLOListElement;
          stickRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        {messages.length === 0 ? (
          <li class="chat__empty">
            <p>Claude Code 세션에 바로 말을 걸 수 있어요.</p>
            <p class="muted">세션이 곁 channel로 이어져 있어야 해요. 대화는 이 창을 닫으면 사라지고 따로 저장하지 않아요.</p>
          </li>
        ) : (
          messages.map((m) => <MessageView key={m.id} m={m} onResend={onResend} />)
        )}
      </ol>

      <form
        class="composer"
        onSubmit={(e) => {
          e.preventDefault();
          send();
        }}
      >
        <label for="chat-input" class="sr-only">
          {effective ? `${effective.project}에 보낼 말` : '보낼 말'}
        </label>
        <textarea
          id="chat-input"
          ref={inputRef}
          class="composer__input"
          rows={2}
          maxLength={MAX_SAY_CHARS}
          placeholder={effective ? `${effective.project}에 말하기…` : '연결된 세션이 없어요'}
          value={text}
          onInput={(e) => setText((e.currentTarget as HTMLTextAreaElement).value)}
          onFocus={() => onListening(true)}
          onBlur={() => onListening(false)}
          onKeyDown={(e) => {
            if (shouldSend(e)) {
              e.preventDefault();
              send();
            }
          }}
          aria-describedby="chat-hint"
        />
        <button type="submit" class="btn btn--primary composer__send" disabled={!text.trim()} aria-label="보내기">
          <IconSend />
        </button>
        <p id="chat-hint" class="composer__hint">
          <span>
            <kbd>Enter</kbd> 보내기 · <kbd>Shift+Enter</kbd> 줄바꿈
          </span>
          <span class="composer__dictate" title="Windows 음성 입력으로 말로 적을 수 있어요">
            <IconMic size={12} /> <kbd>Win+H</kbd> 받아쓰기
          </span>
        </p>
      </form>
    </div>
  );
}
