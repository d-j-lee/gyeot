import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/preact';
import type { Item, ItemKind } from '../types.ts';
import { ItemCard } from './ItemCard.tsx';
import { TurnTab } from './TurnTab.tsx';
import { demoItems } from '../demo/data.ts';
import { ALL_KINDS, TERMINAL_ONLY_NOTE } from '../lib/labels.ts';

afterEach(cleanup);

const NOW = Date.UTC(2026, 10, 26, 5, 30);
const items = demoItems(NOW, 540);
const byKind = (k: ItemKind): Item => items.find((i) => i.kind === k)!;

/** 승인·허락·병합 실행처럼 곁이 사람 대신 결정하는 단추로 읽힐 이름 */
const APPROVE_LIKE = /승인|허락|허용|approve|allow|accept|병합하기|merge/i;

function buttonsOf(container: Element): string[] {
  return [...container.querySelectorAll('button')].map(
    (b) => `${b.textContent ?? ''} ${b.getAttribute('aria-label') ?? ''} ${b.getAttribute('title') ?? ''}`,
  );
}

describe('승인·병합 카드에는 승인 단추가 없다(G-005)', () => {
  for (const kind of ['approval', 'merge'] as const) {
    it(`${kind}: 단추 어디에도 승인 뜻이 없고, 터미널 안내가 보인다`, () => {
      const { container } = render(<ItemCard item={byKind(kind)} now={NOW} onAction={() => {}} />);
      for (const label of buttonsOf(container)) expect(label).not.toMatch(APPROVE_LIKE);
      expect(screen.getByRole('note').textContent).toContain(TERMINAL_ONLY_NOTE);
      // 사람이 할 수 있는 것은 처리함·넘기기·내일 다시·열어 봄 + 점수 펼치기뿐
      const names = [...container.querySelectorAll('button')].map((b) => b.textContent?.trim());
      expect(names.filter((n) => n && !/^\d/.test(n))).toEqual(['처리함', '넘기기', '내일 다시', '열어 봄']);
    });

    it(`${kind}: 넘기기 이유를 펼쳐도 승인 단추는 생기지 않는다`, () => {
      const { container } = render(<ItemCard item={byKind(kind)} now={NOW} onAction={() => {}} />);
      fireEvent.click(screen.getByRole('button', { name: /넘기기/ }));
      fireEvent.click(screen.getByRole('button', { name: /점수/ }));
      for (const label of buttonsOf(container)) expect(label).not.toMatch(APPROVE_LIKE);
    });
  }

  it('명령 미리보기가 선택지(options)를 달고 와도 승인 카드에는 칩을 그리지 않는다', () => {
    const evil: Item = { ...byKind('approval'), options: ['승인', '거절'], decision_id: 'dc_x' };
    const { container } = render(<ItemCard item={evil} now={NOW} onAction={() => {}} onDecide={async () => ({ ok: true })} />);
    expect(container.querySelector('.chip')).toBeNull();
    for (const label of buttonsOf(container)) expect(label).not.toMatch(APPROVE_LIKE);
  });

  it('다른 종류에는 터미널 안내가 없다', () => {
    for (const k of ALL_KINDS.filter((x) => x !== 'approval' && x !== 'merge')) {
      const { unmount } = render(<ItemCard item={byKind(k)} now={NOW} onAction={() => {}} />);
      expect(screen.queryByRole('note')).toBeNull();
      unmount();
    }
  });
});

describe('카드 내용', () => {
  it('종류 배지·프로젝트·상대 시각·점수를 보인다', () => {
    const { container } = render(<ItemCard item={byKind('approval')} now={NOW} onAction={() => {}} />);
    expect(container.querySelector('.badge')!.textContent).toBe('권한 승인');
    expect(screen.getByText('agent-ops-kit')).toBeTruthy();
    expect(screen.getByText('4분째')).toBeTruthy();
    expect(screen.getByRole('button', { name: /점수 95\.3/ })).toBeTruthy();
  });

  it('점수를 누르면 이유를 펼친다', () => {
    render(<ItemCard item={byKind('approval')} now={NOW} onAction={() => {}} />);
    const btn = screen.getByRole('button', { name: /점수/ });
    expect(btn.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(btn);
    expect(btn.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('권한 승인 대기: 기본 95')).toBeTruthy();
  });

  it('긴 본문은 접혀 있고 "더 보기"로 펼친다', () => {
    const { container } = render(<ItemCard item={byKind('unread')} now={NOW} onAction={() => {}} />);
    expect(container.querySelector('.card__body--clamped')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: '더 보기' }));
    expect(container.querySelector('.card__body--clamped')).toBeNull();
    expect(screen.getByRole('button', { name: '접기' })).toBeTruthy();
  });

  it('어제 열린 항목은 "어제"', () => {
    render(<ItemCard item={byKind('unread')} now={NOW} onAction={() => {}} />);
    expect(screen.getByText('어제')).toBeTruthy();
  });
});

describe('동작', () => {
  it('처리함·내일 다시·열어 봄은 계약의 action 으로', () => {
    const onAction = vi.fn();
    const it = byKind('failed');
    render(<ItemCard item={it} now={NOW} onAction={onAction} />);
    fireEvent.click(screen.getByRole('button', { name: '처리함' }));
    fireEvent.click(screen.getByRole('button', { name: '내일 다시' }));
    fireEvent.click(screen.getByRole('button', { name: '열어 봄' }));
    expect(onAction.mock.calls).toEqual([
      [it.id, 'done'],
      [it.id, 'snooze', { minutes: 1440 }],
      [it.id, 'open'],
    ]);
  });

  it('넘기기는 이유(과민·이미 처리·무관)를 고르게 한다', () => {
    const onAction = vi.fn();
    const it = byKind('unread');
    render(<ItemCard item={it} now={NOW} onAction={onAction} />);
    fireEvent.click(screen.getByRole('button', { name: /넘기기/ }));
    expect(onAction).not.toHaveBeenCalled();
    const group = screen.getByRole('group', { name: '넘기는 이유' });
    expect([...group.querySelectorAll('.chip')].map((b) => b.textContent)).toEqual(['과민', '이미 처리', '무관']);
    fireEvent.click(screen.getByRole('button', { name: '과민' }));
    expect(onAction).toHaveBeenCalledWith(it.id, 'dismiss', { reason: '과민' });
  });

  it('넘기기 이유는 취소·Esc 로 접힌다', () => {
    render(<ItemCard item={byKind('unread')} now={NOW} onAction={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: /넘기기/ }));
    fireEvent.keyDown(screen.getByRole('group', { name: '넘기는 이유' }), { key: 'Escape' });
    expect(screen.queryByRole('group', { name: '넘기는 이유' })).toBeNull();
  });

  it('결정 항목은 선택지 칩을 그리고 고르면 decision_id 로 보낸다', async () => {
    const onDecide = vi.fn(async () => ({ ok: true }));
    const it = byKind('decision');
    render(<ItemCard item={it} now={NOW} onAction={() => {}} onDecide={onDecide} />);
    const group = screen.getByRole('group', { name: '고를 수 있는 답' });
    expect([...group.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['지금 적용', '리뷰 뒤에', '취소']);
    fireEvent.click(screen.getByRole('button', { name: '리뷰 뒤에' }));
    expect(onDecide).toHaveBeenCalledWith(it.id, 'dc_5b1e9a', '리뷰 뒤에');
  });

  it('결정을 전하지 못하면 칩 아래에 이유를 보인다', async () => {
    const onDecide = vi.fn(async () => ({ ok: false, message: 'channel이 끊겼어요' }));
    render(<ItemCard item={byKind('decision')} now={NOW} onAction={() => {}} onDecide={onDecide} />);
    fireEvent.click(screen.getByRole('button', { name: '지금 적용' }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('channel이 끊겼어요'));
  });
});

describe('당신 차례 탭', () => {
  it('점수순으로 그리고, 비면 안내를 보인다', () => {
    const shuffled = [...items].reverse();
    const { container, unmount } = render(
      <TurnTab items={[...shuffled].sort((a, b) => b.score - a.score)} now={NOW} seen={{}} highlight={null} onAction={() => {}} onDecide={async () => ({ ok: true })} />,
    );
    const kinds = [...container.querySelectorAll('article')].map((a) => a.getAttribute('data-kind'));
    expect(kinds[0]).toBe('approval');
    expect(screen.getByText(/막힌 일 3건/)).toBeTruthy();
    unmount();
    render(<TurnTab items={[]} now={NOW} seen={{}} highlight={null} onAction={() => {}} onDecide={async () => ({ ok: true })} />);
    expect(screen.getByText('지금은 당신 차례가 없어요')).toBeTruthy();
  });

  it('첫 스냅샷 전에는 "없어요"라고 하지 않는다', () => {
    render(<TurnTab items={[]} synced={false} now={NOW} seen={{}} highlight={null} onAction={() => {}} onDecide={async () => ({ ok: true })} />);
    expect(screen.queryByText('지금은 당신 차례가 없어요')).toBeNull();
    expect(screen.getByText('곁 데몬을 기다리는 중이에요')).toBeTruthy();
  });
});
