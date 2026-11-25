import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render } from '@testing-library/preact';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { Avatar, LEAF_COLORS, LOOK } from './Avatar.tsx';
import { superellipsePath } from '../lib/shape.ts';
import { ALL_MOODS, moodLabel } from '../lib/labels.ts';

afterEach(cleanup);

describe('아바타(avatar-spec)', () => {
  it('잎 색은 규격 표 그대로', () => {
    expect(LEAF_COLORS).toEqual({
      idle: '#9DB89A',
      working: '#7FA7D9',
      waiting: '#F2B544',
      alert: '#E8735A',
      done: '#6CC08B',
      sleep: '#B6A8D9',
      listening: '#5CC2C2',
    });
  });

  for (const mood of ALL_MOODS) {
    it(`${mood}: 기분 클래스·잎 색·이름을 단다`, () => {
      const { container } = render(<Avatar mood={mood} size={112} />);
      const svg = container.querySelector('svg')!;
      expect(svg.getAttribute('class')).toContain(`av--${mood}`);
      expect(svg.getAttribute('aria-label')).toBe(`곁 — ${moodLabel(mood)}`);
      expect(svg.querySelector('.av-leaf-shape')!.getAttribute('fill')).toBe(LEAF_COLORS[mood]);
      expect(svg.getAttribute('width')).toBe('112');
    });
  }

  it('눈 모양: done 은 웃는 호, sleep 은 감은 선, 나머지는 뜬 눈(alert·listening 은 크게)', () => {
    const eyes = (mood: (typeof ALL_MOODS)[number]) => render(<Avatar mood={mood} />).container.querySelector('.av-eyes')!;
    expect(eyes('done').classList.contains('av-eyes--arc')).toBe(true);
    expect(eyes('sleep').classList.contains('av-eyes--closed')).toBe(true);
    const ry = (mood: (typeof ALL_MOODS)[number]) => Number(eyes(mood).querySelector('ellipse')!.getAttribute('ry'));
    expect(ry('alert')).toBeGreaterThan(ry('idle'));
    expect(ry('listening')).toBeGreaterThan(ry('idle'));
  });

  it('입은 몇몇 기분에서만', () => {
    const mouth = (mood: (typeof ALL_MOODS)[number]) => render(<Avatar mood={mood} />).container.querySelector('.av-mouth');
    expect(mouth('idle')).toBeNull();
    expect(mouth('waiting')).toBeNull();
    expect(mouth('done')).not.toBeNull();
  });

  it('done 은 반짝임, sleep 은 z', () => {
    expect(render(<Avatar mood="done" />).container.querySelectorAll('.av-spark')).toHaveLength(3);
    expect(render(<Avatar mood="sleep" />).container.querySelectorAll('.av-z')).toHaveLength(3);
    expect(render(<Avatar mood="idle" />).container.querySelector('.av-fx')).toBeNull();
  });

  it('working 은 아래 왼쪽을 본다, 시선은 덮어쓸 수 있다', () => {
    expect(LOOK.working.x).toBeLessThan(0);
    expect(LOOK.working.y).toBeGreaterThan(0);
    const { container } = render(<Avatar mood="idle" look={{ x: -2, y: -1 }} />);
    expect((container.querySelector('.av-look') as SVGGElement).style.transform).toBe('translate(-2px, -1px)');
  });

  it('단추 안에 넣을 땐 장식으로 숨긴다', () => {
    const svg = render(<Avatar mood="idle" decorative />).container.querySelector('svg')!;
    expect(svg.getAttribute('aria-hidden')).toBe('true');
    expect(svg.getAttribute('role')).toBeNull();
  });

  it('여러 개를 그려도 그라데이션 id 가 겹치지 않는다', () => {
    const { container } = render(
      <div>
        <Avatar mood="idle" />
        <Avatar mood="done" />
      </div>,
    );
    const ids = [...container.querySelectorAll('linearGradient')].map((g) => g.id);
    expect(new Set(ids).size).toBe(2);
  });

  it('몸은 초타원 100:88', () => {
    const d = superellipsePath(60, 72, 42, 36.96, 2.6, 8);
    const nums = (d.match(/-?\d+(\.\d+)?/g) ?? []).map(Number);
    const xs = nums.filter((_, i) => i % 2 === 0);
    const ys = nums.filter((_, i) => i % 2 === 1);
    const w = Math.max(...xs) - Math.min(...xs);
    const h = Math.max(...ys) - Math.min(...ys);
    expect(h / w).toBeCloseTo(0.88, 1);
    expect(d.endsWith('Z')).toBe(true);
  });
});

describe('움직임 줄이기', () => {
  const css = readFileSync(resolve(process.cwd(), 'src/styles/avatar.css'), 'utf8');

  it('prefers-reduced-motion 이면 아바타의 모든 움직임을 끈다', () => {
    const block = css.slice(css.indexOf('@media (prefers-reduced-motion: reduce)'));
    expect(block).toMatch(/\.av \*,\s*\.av\s*\{\s*animation: none !important;\s*transition: none !important;/);
  });

  it('기분마다 움직임 규칙이 있다', () => {
    for (const m of ['working', 'waiting', 'alert', 'done', 'sleep', 'listening']) expect(css).toContain(`.av--${m}`);
    expect(css).toContain('@keyframes av-blink');
  });
});
