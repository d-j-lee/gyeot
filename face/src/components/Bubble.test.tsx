import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/preact';
import { Bubble, LiveAnnouncer } from './Bubble.tsx';
import type { Bubble as BubbleData } from '../lib/bubble.ts';

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const nudge: BubbleData = {
  key: 'd:it_a:1:nudge',
  kind: 'nudge',
  line: 'fsmap 게이트가 멈췄어요',
  reasons: ['점수 80.8 ≥ 80'],
  item: 'it_a',
  project: 'fsmap',
  session: 's',
  at_ms: 1,
  speak: false,
};
const interrupt: BubbleData = { ...nudge, key: 'd:it_b:2:interrupt', kind: 'interrupt', line: '권한 승인을 4분째 기다려요' };

describe('말풍선', () => {
  it('모든 말풍선에 넘기기와 왜?가 있다(G-006)', () => {
    for (const b of [nudge, interrupt, { ...nudge, key: 'r', kind: 'reply' as const }]) {
      const onAck = vi.fn();
      const { unmount } = render(<Bubble bubble={b} tail="down" onAck={onAck} onOpen={() => {}} onHold={() => {}} />);
      fireEvent.click(screen.getByRole('button', { name: '말풍선 넘기기' }));
      expect(onAck).toHaveBeenCalledTimes(1);
      expect(screen.getByRole('button', { name: '왜?' })).toBeTruthy();
      unmount();
    }
  });

  it('왜?는 이유를 펼치고 그동안 시계를 멈춘다', () => {
    const onHold = vi.fn();
    render(<Bubble bubble={nudge} tail="down" onAck={() => {}} onOpen={() => {}} onHold={onHold} />);
    const why = screen.getByRole('button', { name: '왜?' });
    fireEvent.click(why);
    expect(why.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('점수 80.8 ≥ 80')).toBeTruthy();
    expect(onHold).toHaveBeenCalledWith('why', true);
  });

  it('마우스를 올리면 멈추고 떼면 다시 간다', () => {
    const onHold = vi.fn();
    const { container } = render(<Bubble bubble={nudge} tail="down" onAck={() => {}} onOpen={() => {}} onHold={onHold} />);
    fireEvent.mouseEnter(container.querySelector('.bubble')!);
    fireEvent.mouseLeave(container.querySelector('.bubble')!);
    expect(onHold).toHaveBeenCalledWith('hover', true);
    expect(onHold).toHaveBeenCalledWith('hover', false);
  });

  it('보기는 해당 말풍선을 넘겨 준다', () => {
    const onOpen = vi.fn();
    render(<Bubble bubble={nudge} tail="down" onAck={() => {}} onOpen={onOpen} onHold={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: '보기' }));
    expect(onOpen).toHaveBeenCalledWith(nudge);
  });

  it('사라질 때 잠깐 남아 흐려진 뒤 없어진다', () => {
    vi.useFakeTimers();
    const { container, rerender } = render(<Bubble bubble={nudge} tail="down" onAck={() => {}} onOpen={() => {}} onHold={() => {}} />);
    rerender(<Bubble bubble={null} tail="down" onAck={() => {}} onOpen={() => {}} onHold={() => {}} />);
    expect(container.querySelector('.bubble--leaving')).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(container.querySelector('.bubble')).toBeNull();
  });
});

describe('화면 읽기 알림', () => {
  it('nudge 는 polite, interrupt 는 assertive', () => {
    vi.useFakeTimers();
    const { rerender } = render(<LiveAnnouncer bubble={nudge} />);
    act(() => {
      vi.advanceTimersByTime(50);
    });
    expect(screen.getByTestId('live-polite').textContent).toBe(nudge.line);
    expect(screen.getByTestId('live-assertive').textContent).toBe('');
    rerender(<LiveAnnouncer bubble={interrupt} />);
    act(() => {
      vi.advanceTimersByTime(50);
    });
    expect(screen.getByTestId('live-assertive').textContent).toBe(interrupt.line);
    expect(screen.getByTestId('live-assertive').getAttribute('aria-live')).toBe('assertive');
    expect(screen.getByTestId('live-polite').getAttribute('aria-live')).toBe('polite');
  });
});
