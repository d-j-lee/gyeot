import { useEffect } from 'preact/hooks';
import type { DismissReason, Item, ItemAction } from '../types.ts';
import { ItemCard } from './ItemCard.tsx';
import { Avatar } from './Avatar.tsx';

export interface TurnTabProps {
  items: Item[];
  /** 데몬에서 첫 스냅샷을 받았나. 받기 전에는 "없어요"라고 말하지 않는다. */
  synced?: boolean;
  now: number;
  seen: Record<string, string>;
  highlight: string | null;
  onAction: (id: string, action: ItemAction, opts?: { reason?: DismissReason; minutes?: number }) => unknown;
  onDecide: (itemId: string, decisionId: string, choice: string) => Promise<{ ok: boolean; message?: string }>;
}

export function TurnTab({ items, synced = true, now, seen, highlight, onAction, onDecide }: TurnTabProps) {
  useEffect(() => {
    if (!highlight) return;
    document.getElementById(`card-${highlight}`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }, [highlight]);

  if (!synced && items.length === 0) {
    return (
      <div class="empty">
        <Avatar mood="sleep" size={72} decorative />
        <p class="empty__title">곁 데몬을 기다리는 중이에요</p>
        <p class="empty__text">연결되면 당신 차례인 일을 여기에 모아 보여 드릴게요.</p>
      </div>
    );
  }

  if (items.length === 0) {
    return (
      <div class="empty">
        <Avatar mood="idle" size={72} decorative />
        <p class="empty__title">지금은 당신 차례가 없어요</p>
        <p class="empty__text">에이전트가 당신을 기다리게 되면 여기에 모아 둘게요.</p>
      </div>
    );
  }

  const blocking = items.filter((i) => i.blocking).length;
  return (
    <div class="turn">
      <p class="turn__summary">
        {blocking > 0 ? (
          <>
            <strong>막힌 일 {blocking}건</strong> · 전체 {items.length}건 · 점수 높은 순
          </>
        ) : (
          <>전체 {items.length}건 · 점수 높은 순</>
        )}
      </p>
      <ul class="turn__list">
        {items.map((it) => (
          <li key={it.id}>
            <ItemCard
              item={it}
              now={now}
              seen={seen[it.id] === it.signature}
              highlighted={highlight === it.id}
              onAction={onAction}
              onDecide={onDecide}
            />
          </li>
        ))}
      </ul>
    </div>
  );
}
