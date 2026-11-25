import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'preact/hooks';
import type { Mood, Session } from './types.ts';
import type { Controller } from './app/controller.ts';
import type { Bubble as BubbleData } from './lib/bubble.ts';
import { useNow, useObservable } from './app/observable.ts';
import { chatTargets, visibleItems, visibleProposals } from './state/reducer.ts';
import { kindLabel, moodLabel } from './lib/labels.ts';
import { inTauri, setExpanded, setHitRect, unionRect } from './lib/tauri.ts';
import { Avatar } from './components/Avatar.tsx';
import { Bubble, LiveAnnouncer } from './components/Bubble.tsx';
import { Panel, type TabId } from './components/Panel.tsx';
import { StatusLine } from './components/StatusLine.tsx';
import { TurnTab } from './components/TurnTab.tsx';
import { ChatTab } from './components/ChatTab.tsx';
import { SettingsTab } from './components/SettingsTab.tsx';
import { SessionList } from './components/SessionList.tsx';
import { AuthNotice } from './components/AuthNotice.tsx';

export interface AppConfig {
  mode: 'default' | 'overlay';
  initialTab: TabId;
  /** 오버레이에서 처음부터 패널을 펼칠지 */
  initialOpen: boolean;
}

export const OVERLAY_AVATAR_PX = 112;

/** 시선: 오버레이에서 패널이 열리면 위 왼쪽(패널 쪽)을 본다. 정면을 봐야 하는 기분은 그대로. */
function gazeFor(mood: Mood, mode: AppConfig['mode'], open: boolean, hasBubble: boolean) {
  if (mood !== 'idle' && mood !== 'listening') return null;
  if (mode === 'overlay' && open) return { x: -2, y: -1.8 };
  if (mode === 'overlay' && hasBubble) return { x: -0.8, y: -1.8 };
  if (mode === 'default' && hasBubble) return { x: 0, y: 1.6 };
  return null;
}

export function App({ ctl, config }: { ctl: Controller; config: AppConfig }) {
  const state = useObservable(ctl.state);
  const bubble = useObservable(ctl.bubble);
  const toasts = useObservable(ctl.toasts);
  const prefs = useObservable(ctl.prefs);
  const now = useNow(state.clockOffsetMs, 15_000);

  const [tab, setTab] = useState<TabId>(config.initialTab);
  const [open, setOpen] = useState(config.mode === 'default' ? true : config.initialOpen);
  const [target, setTarget] = useState('');
  const [highlight, setHighlight] = useState<string | null>(null);
  const [chatFocus, setChatFocus] = useState(false);

  const avatarBtnRef = useRef<HTMLButtonElement>(null);
  const bubbleWrapRef = useRef<HTMLDivElement>(null);
  const panelWrapRef = useRef<HTMLDivElement>(null);

  const items = useMemo(() => visibleItems(state, now), [state, now]);
  const blockingCount = items.filter((i) => i.blocking).length;
  const proposals = useMemo(() => visibleProposals(state), [state]);
  const sessions = useMemo(() => chatTargets(state.sessions), [state.sessions]);
  const overlay = config.mode === 'overlay';
  // 열쇠를 잃으면 받은 상태가 더는 살아 있지 않다 — 연결 전과 같이 오프라인으로 보인다.
  const authLost = state.conn.status === 'auth';
  const offline = (!state.synced || authLost) && state.conn.status !== 'demo';
  const mood: Mood = state.presence.mood;
  // 오버레이에서 패널이 열려 있으면 말풍선은 패널 뒤에 숨긴다(차례는 그대로).
  const showBubble = !(overlay && open);

  // ── 패널 열고 닫기 ──
  const openPanel = useCallback((t?: TabId, itemId?: string | null) => {
    if (t) setTab(t);
    setHighlight(itemId ?? null);
    setOpen(true);
  }, []);

  const closePanel = useCallback(() => {
    if (!overlay) return;
    setOpen(false);
    setHighlight(null);
    requestAnimationFrame(() => avatarBtnRef.current?.focus());
  }, [overlay]);

  // 대화 탭을 보고 있으면 안 읽은 답 배지를 지운다(§12.4).
  const unread = state.unreadReplies;
  useEffect(() => {
    if (unread > 0 && open && tab === 'chat') ctl.markChatSeen();
  }, [unread, open, tab, ctl]);

  useEffect(() => {
    if (!highlight) return undefined;
    const t = setTimeout(() => setHighlight(null), 2_400);
    return () => clearTimeout(t);
  }, [highlight]);

  // Esc 로 접기, 바깥 누르면 접기(오버레이)
  useEffect(() => {
    if (!overlay) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && open && !e.defaultPrevented) {
        e.preventDefault();
        closePanel();
      }
    };
    const onDown = (e: PointerEvent) => {
      if (!open) return;
      const t = e.target as Node | null;
      const inside = [panelWrapRef.current, avatarBtnRef.current, bubbleWrapRef.current].some((el) => el && t && el.contains(t));
      if (!inside) closePanel();
    };
    // Tauri 오버레이에서는 다른 창을 누르면 우리 창이 초점을 잃는다 → 바깥 클릭으로 본다.
    const onBlur = () => {
      if (open && inTauri()) closePanel();
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('pointerdown', onDown, true);
    window.addEventListener('blur', onBlur);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('pointerdown', onDown, true);
      window.removeEventListener('blur', onBlur);
    };
  }, [overlay, open, closePanel]);

  // 사람이 패널을 열면 탭으로 초점을 옮긴다(오버레이). 처음부터 펼친 채 뜰 때는 옮기지 않는다.
  const firstOpen = useRef(true);
  useEffect(() => {
    const initial = firstOpen.current;
    firstOpen.current = false;
    if (!overlay || !open || initial) return;
    requestAnimationFrame(() => {
      const el = panelWrapRef.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
      el?.focus();
    });
  }, [overlay, open]);

  // ── Tauri: 클릭 받을 영역과 펼침 상태 ──
  useEffect(() => {
    if (overlay) setExpanded(open);
  }, [overlay, open]);

  useLayoutEffect(() => {
    if (!inTauri()) return undefined;
    let raf = 0;
    const measure = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        if (!overlay) {
          setHitRect({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight });
          return;
        }
        const r = unionRect([avatarBtnRef.current, bubbleWrapRef.current, panelWrapRef.current], 4);
        if (r) setHitRect(r);
      });
    };
    measure();
    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(measure) : null;
    for (const el of [avatarBtnRef.current, bubbleWrapRef.current, panelWrapRef.current]) if (el && ro) ro.observe(el);
    window.addEventListener('resize', measure);
    return () => {
      cancelAnimationFrame(raf);
      ro?.disconnect();
      window.removeEventListener('resize', measure);
    };
  }, [overlay, open, bubble?.key, items.length]);

  // ── 말풍선 ──
  const bubbleMeta = useCallback(
    (b: BubbleData) => {
      if (b.kind === 'reply') return `Claude 답장${b.project ? ` · ${b.project}` : ''}`;
      const it = b.item ? state.items[b.item] : undefined;
      if (!it) return b.kind === 'interrupt' ? '급한 일' : null;
      return `${it.project} · ${kindLabel(it.kind)}`;
    },
    [state.items],
  );

  const onBubbleOpen = useCallback(
    (b: BubbleData) => {
      if (b.kind === 'reply') {
        if (b.session) setTarget(b.session);
        openPanel('chat');
      } else {
        openPanel('turn', b.item);
      }
    },
    [openPanel],
  );

  const pickSession = useCallback(
    (s: Session) => {
      setTarget(s.id);
      openPanel('chat');
      setChatFocus(true);
    },
    [openPanel],
  );

  const status = (
    <StatusLine
      mood={offline ? 'idle' : mood}
      line={state.presence.line}
      focusUntil={state.presence.focus_mode_until_ms}
      now={now}
      conn={state.conn}
      onRetry={() => ctl.retryNow()}
    />
  );

  const body =
    tab === 'turn' ? (
      <TurnTab
        items={items}
        synced={state.synced || state.conn.status === 'demo'}
        now={now}
        seen={state.seen}
        highlight={highlight}
        onAction={(id, action, opts) => void ctl.itemAction(id, action, opts)}
        onDecide={(itemId, decisionId, choice) => ctl.decide(itemId, decisionId, choice)}
      />
    ) : tab === 'chat' ? (
      <ChatTab
        sessions={sessions}
        messages={state.chat}
        target={target}
        onTarget={setTarget}
        onSend={(text) => {
          const s = target ? state.sessions.find((x) => x.id === target) : null;
          void ctl.say(text, target || null, s?.project ?? null);
        }}
        onResend={(id) => void ctl.resend(id)}
        onListening={(on) => ctl.setListening(on)}
        autoFocus={chatFocus}
        onAutoFocused={() => setChatFocus(false)}
      />
    ) : (
      <SettingsTab
        now={now}
        focusUntil={state.presence.focus_mode_until_ms}
        prefs={prefs}
        proposals={proposals}
        settings={state.settings}
        version={state.version}
        onFocus={(m) => void ctl.setFocus(m)}
        onPrefs={(p) => ctl.setPrefs(p)}
        onProposal={(id, accept) => void ctl.answerProposal(id, accept)}
        onTestChime={() => ctl.testChime()}
        onTestVoice={() => ctl.testVoice()}
      />
    );

  const panel = (
    <Panel
      id="gyeot-panel"
      variant={overlay ? 'overlay' : 'page'}
      tab={tab}
      onTab={setTab}
      status={status}
      turnCount={items.length}
      blockingCount={blockingCount}
      chatUnread={unread}
      toasts={toasts}
      onDropToast={(id) => ctl.dropToast(id)}
      onClose={overlay ? closePanel : undefined}
      demo={state.conn.status === 'demo'}
      notice={authLost ? <AuthNotice onRetry={() => ctl.retryNow()} /> : null}
    >
      {body}
    </Panel>
  );

  const bubbleEl = (
    <Bubble
      bubble={showBubble ? bubble : null}
      tail={overlay ? 'down' : 'up'}
      meta={bubbleMeta}
      onAck={() => ctl.ackBubble()}
      onOpen={onBubbleOpen}
      onHold={(src, on) => ctl.holdBubble(src, on)}
    />
  );

  const gaze = gazeFor(mood, config.mode, open, Boolean(bubble && showBubble));
  const avatarMood: Mood = offline ? 'idle' : mood;

  if (overlay) {
    const label = `곁 — ${authLost ? '열쇠가 만료됐어요' : moodLabel(avatarMood)}${blockingCount ? `, 막힌 일 ${blockingCount}건` : ''}${unread ? `, 안 읽은 답 ${unread}개` : ''}. 패널 ${open ? '접기' : '열기'}`;
    return (
      <div class="ov">
        <LiveAnnouncer bubble={bubble} />
        {open && (
          <div class="ov-panel" ref={panelWrapRef}>
            {panel}
          </div>
        )}
        <div class="ov-bubble" ref={bubbleWrapRef}>
          {bubbleEl}
        </div>
        <button
          ref={avatarBtnRef}
          type="button"
          class={`ov-avatar${offline ? ' ov-avatar--offline' : ''}${open ? ' ov-avatar--open' : ''}`}
          aria-label={label}
          aria-expanded={open}
          aria-controls="gyeot-panel"
          title={moodLabel(avatarMood)}
          onClick={() => (open ? closePanel() : openPanel())}
        >
          <Avatar mood={avatarMood} size={OVERLAY_AVATAR_PX} look={gaze} decorative />
          {blockingCount > 0 && !open && !(bubble && showBubble) && (avatarMood === 'waiting' || avatarMood === 'alert') && (
            <span class="ov-badge" aria-hidden="true">
              {blockingCount}
            </span>
          )}
          {unread > 0 && !open && <span class="ov-unread" aria-hidden="true" title="안 읽은 답" />}
        </button>
      </div>
    );
  }

  return (
    <div class="page">
      <LiveAnnouncer bubble={bubble} />
      <aside class="aside" aria-label="곁의 얼굴">
        <div class={`aside__face${offline ? ' aside__face--offline' : ''}`}>
          <Avatar mood={avatarMood} size={120} look={gaze} />
        </div>
        <div class="aside__bubble" ref={bubbleWrapRef}>
          {bubbleEl}
        </div>
        <SessionList sessions={state.sessions} now={now} onPick={pickSession} />
      </aside>
      <main class="main" ref={panelWrapRef}>
        {panel}
      </main>
    </div>
  );
}
