import { useEffect, useRef, useState } from 'preact/hooks';
import type { DismissReason, Item, ItemAction } from '../types.ts';
import { agentLabel, ASK_NOT_APPROVAL_NOTE, DISMISS_REASONS, isTerminalOnly, kindLabel, TERMINAL_ONLY_NOTE } from '../lib/labels.ts';
import { formatScore, isLongBody } from '../lib/items.ts';
import { calendarDateKo, clockKo, relativeKo } from '../lib/time.ts';
import { IconChat, IconChevron, IconTerminal } from './Icons.tsx';

export interface ItemCardProps {
  item: Item;
  now: number;
  seen?: boolean;
  highlighted?: boolean;
  onAction: (id: string, action: ItemAction, opts?: { reason?: DismissReason; minutes?: number }) => unknown;
  onDecide?: (itemId: string, decisionId: string, choice: string) => Promise<{ ok: boolean; message?: string }>;
}

type DecisionUi = { state: 'idle' } | { state: 'sending'; choice: string } | { state: 'error'; choice: string; message: string };

export function ItemCard({ item, now, seen = false, highlighted = false, onAction, onDecide }: ItemCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [showReasons, setShowReasons] = useState(false);
  const [dismissing, setDismissing] = useState(false);
  const [decision, setDecision] = useState<DecisionUi>({ state: 'idle' });
  const firstReasonRef = useRef<HTMLButtonElement>(null);
  const dismissBtnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (dismissing) firstReasonRef.current?.focus();
  }, [dismissing]);

  const terminalOnly = isTerminalOnly(item.kind);
  const long = isLongBody(item.body);
  const domId = `card-${item.id}`;
  // §13.5: 선택지가 있는 결정 카드는 Claude 의 질문이지 권한 승인이 아니다 — 질문 위에 늘 밝힌다.
  const asksChoice = item.kind === 'decision' && !!item.options && item.options.length > 0;
  const hasChoices = asksChoice && !!item.decision_id;

  const choose = async (choice: string) => {
    if (!onDecide || !item.decision_id || decision.state === 'sending') return;
    setDecision({ state: 'sending', choice });
    const r = await onDecide(item.id, item.decision_id, choice);
    if (!r.ok) setDecision({ state: 'error', choice, message: r.message ?? '전달하지 못했어요' });
  };

  const closeDismiss = () => {
    setDismissing(false);
    dismissBtnRef.current?.focus();
  };

  const timeTitle = `${calendarDateKo(item.created_ms, now)} ${clockKo(item.created_ms)}에 열림`;

  return (
    <article
      id={domId}
      class={[
        'card',
        `kind--${item.kind}`,
        item.blocking ? 'card--blocking' : '',
        seen ? 'card--seen' : '',
        highlighted ? 'card--highlight' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      aria-labelledby={`${domId}-title`}
      data-kind={item.kind}
    >
      <header class="card__top">
        <span class="badge">{kindLabel(item.kind)}</span>
        <span class="card__where" title={item.cwd}>
          <strong>{item.project || '이름 없는 프로젝트'}</strong>
          <span class="card__agent">{agentLabel(item.agent)}</span>
        </span>
        <span class="card__time">
          <time dateTime={new Date(item.created_ms).toISOString()} title={timeTitle}>
            {relativeKo(item.created_ms, now)}
          </time>
        </span>
        <button
          type="button"
          class="score"
          aria-expanded={showReasons}
          aria-controls={`${domId}-reasons`}
          aria-label={`점수 ${formatScore(item.score)} — 이유 ${showReasons ? '접기' : '보기'}`}
          onClick={() => setShowReasons(!showReasons)}
          title="점수가 왜 이렇게 나왔는지 보기"
        >
          {formatScore(item.score)}
          <IconChevron size={10} open={showReasons} />
        </button>
      </header>

      {asksChoice && (
        <p class="card__ask" id={`${domId}-ask`}>
          <IconChat size={14} />
          <span>{ASK_NOT_APPROVAL_NOTE}</span>
        </p>
      )}

      <h3 class="card__title" id={`${domId}-title`}>
        {item.title || kindLabel(item.kind)}
        {seen && <span class="card__seen">봤음</span>}
      </h3>

      {item.body && (
        <div class={`card__body${terminalOnly ? ' card__body--code' : ''}${long && !expanded ? ' card__body--clamped' : ''}`} id={`${domId}-body`}>
          {item.body}
        </div>
      )}
      {long && (
        <button
          type="button"
          class="linkbtn card__more"
          aria-expanded={expanded}
          aria-controls={`${domId}-body`}
          onClick={() => setExpanded(!expanded)}
        >
          {expanded ? '접기' : '더 보기'}
        </button>
      )}

      {terminalOnly && (
        <p class="card__note" role="note">
          <IconTerminal />
          <span>{TERMINAL_ONLY_NOTE}</span>
        </p>
      )}

      {hasChoices && (
        <div class="choices" role="group" aria-label="고를 수 있는 답" aria-describedby={`${domId}-ask`}>
          {item.options!.map((opt) => {
            const sending = decision.state === 'sending' && decision.choice === opt;
            return (
              <button
                key={opt}
                type="button"
                class={`chip chip--choice${sending ? ' chip--sending' : ''}`}
                disabled={decision.state === 'sending'}
                aria-busy={sending || undefined}
                onClick={() => void choose(opt)}
              >
                {opt}
              </button>
            );
          })}
          {decision.state === 'error' && (
            <p class="inline-hint inline-hint--warn" role="alert">
              ‘{decision.choice}’을 보내지 못했어요 — {decision.message}
            </p>
          )}
        </div>
      )}

      <div class="card__actions">
        <div class="card__btns">
          <button type="button" class="btn btn--sm btn--primary-soft" onClick={() => onAction(item.id, 'done')}>
            처리함
          </button>
          <button
            type="button"
            ref={dismissBtnRef}
            class="btn btn--sm"
            aria-expanded={dismissing}
            aria-controls={`${domId}-dismiss`}
            onClick={() => (dismissing ? closeDismiss() : setDismissing(true))}
          >
            넘기기
            <IconChevron size={12} open={dismissing} />
          </button>
          <button type="button" class="btn btn--sm" onClick={() => onAction(item.id, 'snooze', { minutes: 1440 })}>
            내일 다시
          </button>
          <button type="button" class="btn btn--sm btn--ghost" onClick={() => onAction(item.id, 'open')} disabled={seen}>
            열어 봄
          </button>
        </div>
      </div>

      {dismissing && (
        <div
          class="dismiss"
          id={`${domId}-dismiss`}
          role="group"
          aria-label="넘기는 이유"
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.stopPropagation();
              closeDismiss();
            }
          }}
        >
          <span class="dismiss__label">넘기는 이유</span>
          <div class="dismiss__options">
            {DISMISS_REASONS.map((r, i) => (
              <button
                key={r.value}
                ref={i === 0 ? firstReasonRef : undefined}
                type="button"
                class="chip chip--quiet"
                title={r.hint}
                onClick={() => onAction(item.id, 'dismiss', { reason: r.value })}
              >
                {r.value}
              </button>
            ))}
            <button type="button" class="linkbtn" onClick={closeDismiss}>
              취소
            </button>
          </div>
          <p class="dismiss__hint">‘과민’으로 넘긴 기록만 알림을 줄이자는 제안에 쓰여요.</p>
        </div>
      )}

      {showReasons && (
        <ul class="reasons" id={`${domId}-reasons`} aria-label="점수 이유">
          {item.reasons.length > 0 ? item.reasons.map((r) => <li key={r}>{r}</li>) : <li>데몬이 이유를 보내지 않았어요</li>}
        </ul>
      )}
    </article>
  );
}
