import { afterEach, describe, expect, it, vi } from 'vitest';
import { createBackend } from './create.ts';
import { HttpBackend, type BackendSink } from './backend.ts';
import { DemoBackend } from '../demo/backend.ts';
import { parseBoot } from '../boot.ts';
import type { ConnState } from '../state/reducer.ts';
import type { SseEvent } from '../types.ts';

const loc = (href: string) => new URL(href) as unknown as Location;

afterEach(() => {
  vi.useRealTimers();
  delete (window as Window & { __TAURI__?: unknown }).__TAURI__;
  vi.unstubAllGlobals();
  for (const m of document.head.querySelectorAll('meta[name="gyeot-token"]')) m.remove();
});

describe('백엔드 고르기(main.tsx)', () => {
  it('meta·?t= 에 토큰이 있어도 읽지 않는다 — 요청은 같은 출처 쿠키로만', async () => {
    const meta = document.createElement('meta');
    meta.name = 'gyeot-token';
    meta.content = 'a'.repeat(64);
    document.head.append(meta);
    const fetchSpy = vi.fn(async (_u: string, _i?: RequestInit) => new Response('{"ok":true,"until_ms":null}'));
    vi.stubGlobal('fetch', fetchSpy);
    const href = `http://127.0.0.1:47321/?mode=overlay&t=${'b'.repeat(64)}`;
    const b = createBackend(parseBoot(new URL(href).search), loc(href));
    expect(b).toBeInstanceOf(HttpBackend);
    expect((b as HttpBackend).api.streamUrl()).toBe('/v1/stream');
    await b.focus(true, 30);
    const [url, init] = fetchSpy.mock.calls[0]!;
    expect(url).toBe('/v1/focus');
    expect(init!.credentials).toBe('same-origin');
    const sent = JSON.stringify(init);
    expect(sent).not.toContain('a'.repeat(64));
    expect(sent).not.toContain('b'.repeat(64));
    expect(sent).not.toMatch(/authorization/i);
  });
});

describe('데모(?demo=)는 열쇠 없이 돈다', () => {
  it('네트워크(fetch·EventSource)도 셸 IPC 도 쓰지 않는다', async () => {
    vi.useFakeTimers();
    const fetchSpy = vi.fn();
    const esSpy = vi.fn();
    const invoke = vi.fn(async () => undefined);
    vi.stubGlobal('fetch', fetchSpy);
    vi.stubGlobal('EventSource', esSpy);
    (window as Window & { __TAURI__?: unknown }).__TAURI__ = { core: { invoke } };
    for (const q of ['?demo=panel', '?demo=sleep&mode=overlay', '?demo=reply', '?demo=bubble&t=x&api=http://127.0.0.1:1']) {
      const href = `http://127.0.0.1:47321/${q}`;
      const b = createBackend(parseBoot(new URL(href).search), loc(href));
      expect(b).toBeInstanceOf(DemoBackend);
      const statuses: ConnState[] = [];
      const events: SseEvent[] = [];
      const sink: BackendSink = { status: (c) => statuses.push(c), event: (e) => events.push(e) };
      b.start(sink);
      const done = Promise.all([
        b.focus(true, 30),
        b.say('안녕', null),
        b.itemAction('it_1', 'done'),
        b.decide('dc_1', '지금 적용'),
        b.proposal('pr_1', true),
        b.listening(true),
      ]);
      await vi.advanceTimersByTimeAsync(5_000);
      await done;
      b.retryNow();
      b.stop();
      expect(statuses.map((c) => c.status)).toEqual(['demo']);
      expect(events[0]?.event).toBe('snapshot');
    }
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(esSpy).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });
});
