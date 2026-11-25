import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/preact';
import type { Session } from '../types.ts';
import type { ChatMessage } from '../state/reducer.ts';
import { autoTarget, ChatTab, shouldSend } from './ChatTab.tsx';
import { CHANNEL_MISSING_REASON } from '../demo/data.ts';

afterEach(cleanup);

const sessions: Session[] = [
  { id: 's1', agent: 'claude-code', project: 'agent-ops-kit', cwd: '', state: 'waiting', last_ms: 20, channel: 'ch1' },
  { id: 's2', agent: 'claude-code', project: 'gyeot', cwd: '', state: 'idle', last_ms: 30, channel: 'ch2' },
  { id: 's3', agent: 'codex', project: 'work-atlas', cwd: '', state: 'idle', last_ms: 99, channel: null },
];

function renderChat(over: { messages?: ChatMessage[]; target?: string } = {}) {
  const props = {
    sessions,
    messages: over.messages ?? [],
    target: over.target ?? '',
    onTarget: vi.fn(),
    onSend: vi.fn(),
    onResend: vi.fn(),
    onListening: vi.fn(),
  };
  render(<ChatTab {...props} />);
  return props;
}

const input = () => screen.getByRole('textbox') as HTMLTextAreaElement;

describe('보낼 곳', () => {
  it('자동은 channel 있는 세션 중 가장 최근 것', () => {
    expect(autoTarget(sessions)?.id).toBe('s2');
    expect(autoTarget([sessions[2]!])).toBeNull();
  });

  it('연결됨 표시와 선택', () => {
    const p = renderChat();
    const sel = screen.getByLabelText('보낼 곳') as HTMLSelectElement;
    expect([...sel.options].map((o) => o.textContent)).toEqual([
      '자동 · gyeot',
      'agent-ops-kit · Claude Code · 연결됨',
      'gyeot · Claude Code · 연결됨',
      'work-atlas · Codex',
    ]);
    expect(screen.getByText('연결됨', { selector: '.pill' })).toBeTruthy();
    fireEvent.change(sel, { target: { value: 's3' } });
    expect(p.onTarget).toHaveBeenCalledWith('s3');
  });

  it('channel 없는 세션을 고르면 "연결 안 됨"', () => {
    renderChat({ target: 's3' });
    expect(screen.getByText('연결 안 됨')).toBeTruthy();
  });
});

describe('입력', () => {
  it('Enter 로 보내고 입력창을 비운다', () => {
    const p = renderChat();
    fireEvent.input(input(), { target: { value: '  테스트 결과 알려 줘  ' } });
    fireEvent.keyDown(input(), { key: 'Enter' });
    expect(p.onSend).toHaveBeenCalledWith('테스트 결과 알려 줘');
    expect(input().value).toBe('');
  });

  it('Shift+Enter 는 줄바꿈(보내지 않음)', () => {
    const p = renderChat();
    fireEvent.input(input(), { target: { value: '첫 줄' } });
    fireEvent.keyDown(input(), { key: 'Enter', shiftKey: true });
    expect(p.onSend).not.toHaveBeenCalled();
  });

  it('한글 조합 중(IME)의 Enter 는 보내지 않는다', () => {
    const p = renderChat();
    fireEvent.input(input(), { target: { value: '안녕하세' } });
    fireEvent.keyDown(input(), { key: 'Enter', isComposing: true });
    fireEvent.keyDown(input(), { key: 'Enter', keyCode: 229 });
    expect(p.onSend).not.toHaveBeenCalled();
    expect(shouldSend({ key: 'Enter', shiftKey: false, isComposing: false, keyCode: 13, altKey: false, ctrlKey: false, metaKey: false })).toBe(true);
  });

  it('빈 글은 보내지 않는다', () => {
    const p = renderChat();
    fireEvent.input(input(), { target: { value: '   ' } });
    fireEvent.keyDown(input(), { key: 'Enter' });
    expect(p.onSend).not.toHaveBeenCalled();
  });

  it('입력창에 초점이 들어오고 나갈 때 듣는 중을 알린다', () => {
    const p = renderChat();
    fireEvent.focus(input());
    fireEvent.blur(input());
    expect(p.onListening.mock.calls).toEqual([[true], [false]]);
  });

  it('Win+H 받아쓰기 안내가 있다', () => {
    renderChat();
    expect(screen.getByText(/받아쓰기/).textContent).toContain('Win+H');
  });
});

describe('메시지', () => {
  it('내 글과 Claude 답을 보이고, 전달 못 한 이유와 시작 명령 복사를 안내한다', () => {
    const p = renderChat({
      messages: [
        { id: 'm1', role: 'me', text: '안녕', at_ms: 1, session: null, project: null, status: 'undelivered', deliveredTo: null, reason: CHANNEL_MISSING_REASON },
        { id: 'r1', role: 'claude', text: '네, 듣고 있어요', at_ms: 2, session: 's1', project: 'agent-ops-kit' },
      ],
    });
    expect(screen.getByText('세션에 전달하지 못했어요')).toBeTruthy();
    expect(screen.getByText('--dangerously-load-development-channels server:gyeot', { selector: 'code' })).toBeTruthy();
    expect(screen.getByRole('button', { name: /시작 명령 복사/ })).toBeTruthy();
    expect(screen.getByText('네, 듣고 있어요')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '다시 보내기' }));
    expect(p.onResend).toHaveBeenCalledWith('m1');
  });

  it('넘긴 글은 "세션에 전달 요청함"(읽었다는 뜻이 아님, §10.1)', () => {
    renderChat({
      messages: [{ id: 'm1', role: 'me', text: '안녕', at_ms: 1, session: 's1', project: 'agent-ops-kit', status: 'sent', deliveredTo: 's1' }],
    });
    expect(screen.getByText('agent-ops-kit 세션에 전달 요청함')).toBeTruthy();
    expect(screen.queryByText(/전달됨/)).toBeNull();
    expect(screen.queryByText(/답이 없으면/)).toBeNull();
  });

  it('30초 동안 답이 없으면 channel 플래그 확인 안내를 보이고, 답이 오면 거둔다', () => {
    const base = { id: 'm1', role: 'me' as const, text: '안녕', at_ms: 1, session: 's1', project: 'agent-ops-kit', status: 'sent' as const, deliveredTo: 's1' };
    renderChat({ messages: [{ ...base, noReply: true }] });
    const hint = screen.getByText(/답이 없으면 그 세션을/);
    expect(hint.textContent).toBe('답이 없으면 그 세션을 claude --dangerously-load-development-channels server:gyeot 로 시작했는지 확인하세요.');
    expect(hint.querySelector('code')!.textContent).toBe('claude --dangerously-load-development-channels server:gyeot');
    cleanup();
    renderChat({ messages: [{ ...base, noReply: false, answered: true }] });
    expect(screen.queryByText(/답이 없으면/)).toBeNull();
    expect(screen.getByText(/답 받음/)).toBeTruthy();
  });
});
