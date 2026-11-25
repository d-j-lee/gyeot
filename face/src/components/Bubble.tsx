import { useEffect, useRef, useState } from 'preact/hooks';
import type { Bubble as BubbleData } from '../lib/bubble.ts';

export interface BubbleProps {
  bubble: BubbleData | null;
  /** 꼬리 방향: 아바타가 아래에 있으면 down, 위에 있으면 up */
  tail: 'down' | 'up';
  /** 말풍선 위쪽 작은 줄(프로젝트 · 종류) */
  meta?: (b: BubbleData) => string | null;
  onAck: () => void;
  onOpen: (b: BubbleData) => void;
  onHold: (source: string, on: boolean) => void;
}

const LEAVE_MS = 180;

/**
 * 말풍선. 넘기기·왜?가 늘 있다(G-006). 내용은 화면 읽기 프로그램에
 * 따로 알린다(LiveAnnouncer) — 이 요소 자체는 live 영역이 아니다.
 */
export function Bubble({ bubble, tail, meta, onAck, onOpen, onHold }: BubbleProps) {
  const [shown, setShown] = useState<BubbleData | null>(bubble);
  const [leaving, setLeaving] = useState(false);
  const [why, setWhy] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (bubble) {
      setShown(bubble);
      setLeaving(false);
      return undefined;
    }
    if (!shown) return undefined;
    setLeaving(true);
    const t = setTimeout(() => {
      setShown(null);
      setLeaving(false);
    }, LEAVE_MS);
    return () => clearTimeout(t);
  }, [bubble]);

  // 새 말풍선마다 "왜?"를 접고, 앞 말풍선이 걸어 둔 멈춤을 푼다.
  const key = shown?.key ?? null;
  useEffect(() => {
    setWhy(false);
    return () => {
      onHold('hover', false);
      onHold('focus', false);
      onHold('why', false);
    };
  }, [key]);

  useEffect(() => {
    onHold('why', why);
  }, [why]);

  if (!shown) return null;
  const metaText = meta?.(shown) ?? null;
  const reasonsId = `bubble-why-${shown.key.replace(/[^a-zA-Z0-9_-]/g, '')}`;
  const openLabel = shown.kind === 'reply' ? '대화 보기' : '보기';

  return (
    <div
      ref={rootRef}
      class={`bubble bubble--${shown.kind} bubble--tail-${tail}${leaving ? ' bubble--leaving' : ''}`}
      data-kind={shown.kind}
      onMouseEnter={() => onHold('hover', true)}
      onMouseLeave={() => onHold('hover', false)}
      onFocusIn={() => onHold('focus', true)}
      onFocusOut={(e) => {
        const next = (e as FocusEvent).relatedTarget as Node | null;
        if (!next || !rootRef.current?.contains(next)) onHold('focus', false);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Escape' && why) {
          e.stopPropagation();
          setWhy(false);
        }
      }}
    >
      {metaText && (
        <p class="bubble__meta">
          <span class="bubble__dot" aria-hidden="true" />
          {metaText}
        </p>
      )}
      <p class="bubble__line">{shown.line}</p>
      {why && (
        <ul class="bubble__reasons" id={reasonsId}>
          {shown.reasons.length > 0 ? shown.reasons.map((r) => <li key={r}>{r}</li>) : <li>데몬이 이유를 보내지 않았어요</li>}
        </ul>
      )}
      <div class="bubble__actions">
        <button type="button" class="btn btn--xs btn--ghost" onClick={() => onOpen(shown)}>
          {openLabel}
        </button>
        <button
          type="button"
          class="btn btn--xs btn--ghost"
          aria-expanded={why}
          aria-controls={reasonsId}
          onClick={() => setWhy(!why)}
        >
          왜?
        </button>
        <span class="spacer" />
        <button
          type="button"
          class="btn btn--xs"
          onClick={onAck}
          aria-label="말풍선 넘기기"
          title="말풍선만 닫아요. 항목은 ‘당신 차례’에 남아요"
        >
          넘기기
        </button>
      </div>
      <span class="bubble__tail" aria-hidden="true" />
    </div>
  );
}

/**
 * 화면 읽기 프로그램 알림. nudge·reply 는 polite, interrupt 는 assertive.
 * 처음부터 있는 두 영역에 글을 써 넣어야 안정적으로 읽힌다.
 */
export function LiveAnnouncer({ bubble }: { bubble: BubbleData | null }) {
  const [polite, setPolite] = useState('');
  const [assertive, setAssertive] = useState('');
  const last = useRef<string | null>(null);
  useEffect(() => {
    if (!bubble || bubble.key === last.current) return;
    last.current = bubble.key;
    const text = bubble.kind === 'reply' ? `Claude 답장: ${bubble.line}` : bubble.line;
    if (bubble.kind === 'interrupt') {
      setAssertive('');
      setTimeout(() => setAssertive(text), 30);
    } else {
      setPolite('');
      setTimeout(() => setPolite(text), 30);
    }
  }, [bubble]);
  return (
    <div class="sr-only">
      <div aria-live="polite" role="status" data-testid="live-polite">
        {polite}
      </div>
      <div aria-live="assertive" role="alert" data-testid="live-assertive">
        {assertive}
      </div>
    </div>
  );
}
