// 색 토큰의 글자 대비가 WCAG AA(본문 4.5:1)를 지키는지 잰다.
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// vitest 는 CSS import 를 비워 두므로 파일을 직접 읽는다(npm test 는 face/ 에서 돈다).
const css = readFileSync(resolve(process.cwd(), 'src/styles/theme.css'), 'utf8');

function block(selector: string, from = 0): string {
  const start = css.indexOf(selector, from);
  if (start < 0) throw new Error(`${selector} 없음`);
  const open = css.indexOf('{', start);
  let depth = 0;
  for (let i = open; i < css.length; i++) {
    if (css[i] === '{') depth++;
    else if (css[i] === '}' && --depth === 0) return css.slice(open + 1, i);
  }
  throw new Error('닫는 괄호 없음');
}

function vars(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of text.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) out[m[1]!] = m[2]!.toLowerCase();
  return out;
}

function lum(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0]! + 0.7152 * c[1]! + 0.0722 * c[2]!;
}

export function contrast(a: string, b: string): number {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x! + 0.05) / (y! + 0.05);
}

const light = vars(block(':root {'));
const dark = { ...light, ...vars(block(":root[data-theme='dark']")) };
const darkMedia = vars(block(":root:not([data-theme='light'])"));

const KINDS = ['approval', 'merge', 'decision', 'failed', 'volatile', 'stall', 'followup', 'unread', 'context'];

/** [글자, 바탕] 쌍 — 화면에서 실제로 만나는 조합 */
const PAIRS: [string, string][] = [
  ['text', 'bg'],
  ['text', 'surface'],
  ['text', 'surface-2'],
  ['text', 'surface-3'],
  ['text-2', 'surface'],
  ['text-2', 'surface-2'],
  ['text-2', 'bg'],
  ['text-3', 'surface'],
  ['text-3', 'bg'],
  ['text-3', 'surface-2'],
  ['on-accent', 'accent'],
  ['accent-text', 'surface'],
  ['accent-text', 'accent-soft'],
  ['ok', 'ok-soft'],
  ['warn', 'surface'],
  ['text', 'warn-soft'],
  ['danger', 'surface'],
  ['text', 'me-bubble'],
  ...KINDS.map((k) => [`k-${k}-fg`, `k-${k}-bg`] as [string, string]),
  ...KINDS.map((k) => [`k-${k}-fg`, 'surface'] as [string, string]),
];

describe('WCAG AA 글자 대비', () => {
  for (const [name, theme] of [
    ['밝은 모드', light],
    ['어두운 모드', dark],
  ] as const) {
    it(`${name}: 모든 쌍이 4.5:1 이상`, () => {
      const bad: string[] = [];
      for (const [fg, bg] of PAIRS) {
        const a = theme[fg];
        const b = theme[bg];
        expect(a, `--${fg}`).toBeDefined();
        expect(b, `--${bg}`).toBeDefined();
        const r = contrast(a!, b!);
        if (r < 4.5) bad.push(`--${fg} on --${bg}: ${r.toFixed(2)}`);
      }
      expect(bad).toEqual([]);
    });
  }

  it('시스템 어두운 모드와 ?theme=dark 의 색이 같다', () => {
    expect(darkMedia).toEqual(vars(block(":root[data-theme='dark']")));
  });
});

// 결정 카드의 선택지 칩(§13.5): 승인 단추처럼 보이는 강조색(accent·ok·종류색)을 쓰지 않는다.
const appCss = readFileSync(resolve(process.cwd(), 'src/styles/app.css'), 'utf8');
const NEUTRAL = new Set(['text', 'surface', 'surface-2', 'surface-3', 'border', 'border-strong']);
const COLOR_PROPS = /^(color|background|background-color|border|border-color|border-[a-z]+-color|box-shadow|outline|outline-color)$/;

function rulesFor(selectorTest: (sel: string) => boolean): { sel: string; decls: [string, string][] }[] {
  const out: { sel: string; decls: [string, string][] }[] = [];
  for (const m of appCss.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const sel = m[1]!.replace(/\/\*[\s\S]*?\*\//g, '').trim();
    if (!sel.split(',').some((s) => selectorTest(s.trim()))) continue;
    const decls = m[2]!
      .split(';')
      .map((d) => d.trim())
      .filter(Boolean)
      .map((d) => {
        const i = d.indexOf(':');
        return [d.slice(0, i).trim(), d.slice(i + 1).trim()] as [string, string];
      });
    out.push({ sel, decls });
  }
  return out;
}

describe('결정 선택지 칩은 중립색(§13.5)', () => {
  // 선택지 칩은 class="chip chip--choice" 다. .chip 바탕 규칙과 .chip--choice 규칙 모두 본다(.chip--quiet 는 넘기기 이유).
  const choiceRules = rulesFor((s) => /^\.chip(--choice|--sending)?(?![\w-])/.test(s) || s.includes('.chip--choice'));

  it('규칙이 있다(.chip--choice 포함)', () => {
    expect(choiceRules.some((r) => r.sel.includes('.chip--choice'))).toBe(true);
  });

  it('색은 중립 토큰(글자·바탕·테두리)만 — accent·ok·종류색·직접 쓴 색 없음', () => {
    const bad: string[] = [];
    for (const r of choiceRules) {
      for (const [prop, value] of r.decls) {
        if (!COLOR_PROPS.test(prop)) continue;
        if (/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i.test(value)) bad.push(`${r.sel} { ${prop}: ${value} }`);
        for (const v of value.matchAll(/var\(--([\w-]+)\)/g)) {
          if (!NEUTRAL.has(v[1]!)) bad.push(`${r.sel} { ${prop}: ${value} }`);
        }
      }
    }
    expect(bad).toEqual([]);
  });

  it('칩의 글자·바탕 쌍이 AA 시험 목록에 있다', () => {
    for (const bg of ['surface', 'surface-2', 'surface-3']) {
      expect(PAIRS.some(([f, b]) => f === 'text' && b === bg), bg).toBe(true);
    }
  });
});
