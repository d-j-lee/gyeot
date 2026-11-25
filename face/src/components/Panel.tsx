import { useRef } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import type { Toast } from '../app/controller.ts';
import { IconClose } from './Icons.tsx';

export type TabId = 'turn' | 'chat' | 'settings';

export const TABS: readonly { id: TabId; label: string }[] = [
  { id: 'turn', label: '당신 차례' },
  { id: 'chat', label: '대화' },
  { id: 'settings', label: '설정' },
];

export interface PanelProps {
  variant: 'overlay' | 'page';
  tab: TabId;
  onTab: (t: TabId) => void;
  status: ComponentChildren;
  turnCount: number;
  blockingCount: number;
  /** 집중 모드 등으로 말풍선 없이 들어온 답의 수(§12.4) */
  chatUnread?: number;
  toasts: Toast[];
  onDropToast: (id: number) => void;
  onClose?: () => void;
  /** 데모 모드 표시 */
  demo?: boolean;
  /** 탭 위에 띄우는 안내(열쇠 만료 등) */
  notice?: ComponentChildren;
  children: ComponentChildren;
  id?: string;
}

export function Panel(p: PanelProps) {
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const onTabKey = (e: KeyboardEvent, i: number) => {
    let next = -1;
    if (e.key === 'ArrowRight') next = (i + 1) % TABS.length;
    else if (e.key === 'ArrowLeft') next = (i - 1 + TABS.length) % TABS.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = TABS.length - 1;
    if (next < 0) return;
    e.preventDefault();
    p.onTab(TABS[next]!.id);
    tabRefs.current[next]?.focus();
  };

  return (
    <section
      id={p.id}
      class={`panel panel--${p.variant}`}
      aria-label="곁 패널"
      role={p.variant === 'overlay' ? 'dialog' : undefined}
      aria-modal={p.variant === 'overlay' ? 'false' : undefined}
    >
      <header class="panel__head">
        <div class="panel__titlebar">
          <h1 class="panel__title">
            곁
            {p.demo && (
              <span class="tag tag--demo" title="데몬 없이 표본 데이터로 보여요">
                데모
              </span>
            )}
          </h1>
          <div class="panel__status">{p.status}</div>
          {p.onClose && (
            <button type="button" class="iconbtn" onClick={p.onClose} aria-label="패널 접기 (Esc)" title="접기 (Esc)">
              <IconClose />
            </button>
          )}
        </div>
        <div class="tabs" role="tablist" aria-label="곁 패널 탭">
          {TABS.map((t, i) => {
            const selected = p.tab === t.id;
            return (
              <button
                key={t.id}
                ref={(el) => {
                  tabRefs.current[i] = el;
                }}
                type="button"
                role="tab"
                id={`tab-${t.id}`}
                class="tab"
                aria-selected={selected}
                aria-controls={`tabpanel-${t.id}`}
                tabIndex={selected ? 0 : -1}
                onClick={() => p.onTab(t.id)}
                onKeyDown={(e) => onTabKey(e, i)}
              >
                {t.label}
                {t.id === 'turn' && p.turnCount > 0 && (
                  <span class={`tab__count${p.blockingCount > 0 ? ' tab__count--hot' : ''}`} aria-label={`${p.turnCount}건`}>
                    {p.turnCount}
                  </span>
                )}
                {t.id === 'chat' && (p.chatUnread ?? 0) > 0 && (
                  <span class="tab__count tab__count--unread" aria-label={`안 읽은 답 ${p.chatUnread}개`}>
                    {p.chatUnread}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      </header>
      {p.notice}
      <div
        class={`panel__body panel__body--${p.tab}`}
        role="tabpanel"
        id={`tabpanel-${p.tab}`}
        aria-labelledby={`tab-${p.tab}`}
        tabIndex={p.tab === 'chat' ? -1 : 0}
      >
        {p.children}
      </div>
      <div class="toasts" role="status" aria-live="polite">
        {p.toasts.map((t) => (
          <div key={t.id} class="toast">
            <span>{t.text}</span>
            <button type="button" class="iconbtn iconbtn--sm" aria-label="알림 닫기" onClick={() => p.onDropToast(t.id)}>
              <IconClose size={12} />
            </button>
          </div>
        ))}
      </div>
    </section>
  );
}
