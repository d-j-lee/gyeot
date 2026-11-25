import { describe, expect, it } from 'vitest';
import { parseBoot } from './boot.ts';

describe('주소 읽기', () => {
  it('기본은 브라우저 탭 화면, 데몬에 연결', () => {
    expect(parseBoot('')).toEqual({ mode: 'default', demo: null, tab: 'turn', open: false, theme: null, reduceMotion: false });
  });

  it('?mode=overlay', () => {
    expect(parseBoot('?mode=overlay').mode).toBe('overlay');
    expect(parseBoot('?mode=nope').mode).toBe('default');
  });

  it('?demo=<기분> 은 그 기분으로, 장면 이름은 장면으로', () => {
    expect(parseBoot('?demo=sleep').demo).toEqual({ scene: 'mood', mood: 'sleep' });
    expect(parseBoot('?demo=panel')).toMatchObject({ demo: { scene: 'panel', mood: 'waiting' }, open: true });
    expect(parseBoot('?demo=bubble').demo).toEqual({ scene: 'bubble', mood: 'waiting' });
    expect(parseBoot('?demo=sheet').demo?.scene).toBe('sheet');
    expect(parseBoot('?demo=').demo).toEqual({ scene: 'mood', mood: 'waiting' });
  });

  it('탭·색·움직임', () => {
    expect(parseBoot('?tab=chat&theme=dark&motion=reduce')).toMatchObject({ tab: 'chat', theme: 'dark', reduceMotion: true });
    expect(parseBoot('?tab=bogus&theme=blue')).toMatchObject({ tab: 'turn', theme: null });
    expect(parseBoot('?mode=overlay&open=1').open).toBe(true);
  });
});
