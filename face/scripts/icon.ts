// 앱 아이콘(오리지널 "곁", idle 표정, 세이지 잎, 투명 배경)을 SVG 로 만든다.
//   node scripts/icon.ts   → public/favicon.svg, public/app-icon.svg(1024 원본)
// 아바타 컴포넌트와 같은 기하(src/lib/shape.ts)를 쓴다.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { superellipsePath } from '../src/lib/shape.ts';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..', 'public');

export function iconSvg(size: number): string {
  const body = superellipsePath(60, 72, 42, 36.96, 2.6);
  const ex = (2 * 42 * 0.28) / 2;
  // 아이콘은 그림자 없이 가운데에 둔다. 크게 줄이고 늘리므로 외곽선은 얇게.
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="6 6 108 108">
  <defs>
    <linearGradient id="b" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#F4EFE6"/><stop offset="0.7" stop-color="#F4EFE6"/><stop offset="1" stop-color="#E6DDCF"/>
    </linearGradient>
  </defs>
  <path d="M78 40 C77.6 32 79.6 26.4 83 21.4" fill="none" stroke="#7E9873" stroke-width="2.4" stroke-linecap="round"/>
  <path d="M83 21.4 C84.6 13.6 93 9.6 101.6 11.6 C99 19.4 91.6 23.6 83 21.4 Z" fill="#9DB89A" stroke="#3A3530" stroke-opacity="0.14" stroke-width="0.6"/>
  <path d="M84.4 20.4 Q92.4 16.6 99.2 12.6" fill="none" stroke="#FFFFFF" stroke-opacity="0.45" stroke-width="0.9" stroke-linecap="round"/>
  <path d="${body}" fill="url(#b)" stroke="#3A3530" stroke-opacity="0.16" stroke-width="0.9"/>
  <circle cx="${60 - 21.5}" cy="75.5" r="4.6" fill="#F2B8A0" opacity="0.45"/>
  <circle cx="${60 + 21.5}" cy="75.5" r="4.6" fill="#F2B8A0" opacity="0.45"/>
  <ellipse cx="${60 - ex}" cy="66" rx="3.6" ry="5.2" fill="#2B2622"/>
  <ellipse cx="${60 + ex}" cy="66" rx="3.6" ry="5.2" fill="#2B2622"/>
  <circle cx="${60 - ex + 1.25}" cy="64" r="1.3" fill="#FFFFFF"/>
  <circle cx="${60 + ex + 1.25}" cy="64" r="1.3" fill="#FFFFFF"/>
</svg>
`;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'favicon.svg'), iconSvg(32));
  writeFileSync(join(out, 'app-icon.svg'), iconSvg(1024));
  console.log('public/favicon.svg, public/app-icon.svg 를 만들었어요');
}
