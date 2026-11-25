import { useId } from 'preact/hooks';
import type { Mood } from '../types.ts';
import { moodLabel } from '../lib/labels.ts';
import { superellipsePath } from '../lib/shape.ts';

export { superellipsePath };

// docs/avatar-spec.md 를 따른 오리지널 아바타 "곁".
// 몸은 초타원(n≈2.6) 조약돌, 가로 100 : 세로 88. 머리 오른쪽 위의 새싹 잎이 상태색이다.

export const LEAF_COLORS: Record<Mood, string> = {
  idle: '#9DB89A',
  working: '#7FA7D9',
  waiting: '#F2B544',
  alert: '#E8735A',
  done: '#6CC08B',
  sleep: '#B6A8D9',
  listening: '#5CC2C2',
};

/** 기분별 기본 시선(아바타 좌표 단위). working 은 아래 왼쪽(읽는 중). */
export const LOOK: Record<Mood, { x: number; y: number }> = {
  idle: { x: 0, y: 0 },
  working: { x: -2.2, y: 2.2 },
  waiting: { x: 0, y: 0 },
  alert: { x: 0, y: 0 },
  done: { x: 0, y: 0 },
  sleep: { x: 0, y: 0 },
  listening: { x: 0, y: -0.6 },
};

// 몸 기하(viewBox 0 0 120 120)
const CX = 60;
const CY = 72;
const A = 42; // 가로 반지름 → 너비 84
const B = 36.96; // 세로 반지름 → 높이 73.9 (100:88)
const EYE_Y = 66;
const EYE_DX = (2 * A * 0.28) / 2; // 눈 간격 = 몸 너비의 28%

const BODY_PATH = superellipsePath(CX, CY, A, B, 2.6);
const INK = '#2B2622';

function Eyes({ mood }: { mood: Mood }) {
  const xs = [CX - EYE_DX, CX + EYE_DX];
  if (mood === 'done') {
    // 웃는 호 ^ ^
    return (
      <g class="av-eyes av-eyes--arc">
        {xs.map((x) => (
          <path
            key={x}
            d={`M${x - 4.2} ${EYE_Y + 1.6} Q${x} ${EYE_Y - 4.4} ${x + 4.2} ${EYE_Y + 1.6}`}
            fill="none"
            stroke={INK}
            stroke-width="2.3"
            stroke-linecap="round"
          />
        ))}
      </g>
    );
  }
  if (mood === 'sleep') {
    // 감은 선(아래로 처진 호)
    return (
      <g class="av-eyes av-eyes--closed">
        {xs.map((x) => (
          <path
            key={x}
            d={`M${x - 4.2} ${EYE_Y} Q${x} ${EYE_Y + 3} ${x + 4.2} ${EYE_Y}`}
            fill="none"
            stroke={INK}
            stroke-width="2.1"
            stroke-linecap="round"
          />
        ))}
      </g>
    );
  }
  const wide = mood === 'alert' || mood === 'listening';
  const rx = wide ? 4.3 : 3.6;
  const ry = wide ? 6.3 : 5.2;
  return (
    <g class="av-eyes av-eyes--open">
      {xs.map((x) => (
        <g key={x}>
          <ellipse cx={x} cy={EYE_Y} rx={rx} ry={ry} fill={INK} />
          <circle cx={x + 1.25} cy={EYE_Y - ry * 0.38} r={wide ? 1.55 : 1.3} fill="#FFFFFF" />
        </g>
      ))}
    </g>
  );
}

function Mouth({ mood }: { mood: Mood }) {
  const y = 80.5;
  switch (mood) {
    case 'working':
      return <path class="av-mouth" d={`M${CX - 2.6} ${y} H${CX + 2.6}`} stroke={INK} stroke-width="1.6" stroke-linecap="round" opacity="0.75" />;
    case 'done':
      return <path class="av-mouth" d={`M${CX - 4} ${y - 1} Q${CX} ${y + 3.6} ${CX + 4} ${y - 1}`} fill="none" stroke={INK} stroke-width="1.7" stroke-linecap="round" />;
    case 'alert':
      return <ellipse class="av-mouth" cx={CX} cy={y + 0.6} rx="2.2" ry="2.7" fill={INK} opacity="0.85" />;
    case 'listening':
      return <ellipse class="av-mouth" cx={CX} cy={y + 0.4} rx="1.6" ry="1.9" fill={INK} opacity="0.8" />;
    default:
      return null;
  }
}

function Sparkles() {
  const star = 'M0 -4.2 L1.1 -1.1 L4.2 0 L1.1 1.1 L0 4.2 L-1.1 1.1 L-4.2 0 L-1.1 -1.1 Z';
  return (
    <g class="av-fx av-sparkles" aria-hidden="true">
      {/* 자리는 바깥 g 의 transform 속성, 반짝임은 안쪽 path 의 CSS transform(서로 덮어쓰지 않게) */}
      <g transform="translate(110 34)">
        <path class="av-spark av-spark--1" d={star} />
      </g>
      <g transform="translate(12 44) scale(0.75)">
        <path class="av-spark av-spark--2" d={star} />
      </g>
      <g transform="translate(108 13) scale(0.6)">
        <path class="av-spark av-spark--3" d={star} />
      </g>
    </g>
  );
}

function Zzz() {
  return (
    <g class="av-fx av-zzz" aria-hidden="true">
      <text class="av-z av-z--1" x="38" y="34" font-size="10">z</text>
      <text class="av-z av-z--2" x="30" y="24" font-size="12">z</text>
      <text class="av-z av-z--3" x="21" y="13" font-size="14">z</text>
    </g>
  );
}

export interface AvatarProps {
  mood: Mood;
  /** 표시 크기(px). 오버레이는 96–128. */
  size?: number;
  /** 시선 덮어쓰기(아바타 좌표, ±3 정도) */
  look?: { x: number; y: number } | null;
  /** 단추 안에 넣을 때처럼 이름을 바깥에서 붙이면 true */
  decorative?: boolean;
  class?: string;
}

export function Avatar({ mood, size = 112, look, decorative = false, class: cls }: AvatarProps) {
  const uid = useId().replace(/[^a-zA-Z0-9_-]/g, '');
  const gradId = `av-body-${uid}`;
  const leaf = LEAF_COLORS[mood] ?? LEAF_COLORS.idle;
  const gaze = look ?? LOOK[mood] ?? LOOK.idle;
  const label = `곁 — ${moodLabel(mood)}`;
  return (
    <svg
      class={`av av--${mood}${cls ? ` ${cls}` : ''}`}
      viewBox="0 0 120 120"
      width={size}
      height={size}
      role={decorative ? undefined : 'img'}
      aria-label={decorative ? undefined : label}
      aria-hidden={decorative ? 'true' : undefined}
      focusable="false"
      data-mood={mood}
      style={{ '--leaf': leaf } as Record<string, string>}
    >
      {!decorative && <title>{label}</title>}
      <defs>
        <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="#F4EFE6" />
          <stop offset="0.7" stop-color="#F4EFE6" />
          <stop offset="1" stop-color="#E6DDCF" />
        </linearGradient>
      </defs>

      <ellipse class="av-shadow" cx={CX} cy="111.5" rx="30" ry="4.6" fill="#000" opacity="0.1" />

      <g class="av-rig">
        <g class="av-breath">
          <g class="av-sprout">
            <path class="av-stem" d="M78 40 C77.6 32 79.6 26.4 83 21.4" fill="none" stroke="#7E9873" stroke-width="2.4" stroke-linecap="round" />
            <g class="av-leaf">
              <path
                class="av-leaf-shape"
                d="M83 21.4 C84.6 13.6 93 9.6 101.6 11.6 C99 19.4 91.6 23.6 83 21.4 Z"
                fill={leaf}
                stroke="#3A3530"
                stroke-opacity="0.16"
                stroke-width="1"
              />
              <path d="M84.4 20.4 Q92.4 16.6 99.2 12.6" fill="none" stroke="#FFFFFF" stroke-opacity="0.45" stroke-width="0.9" stroke-linecap="round" />
            </g>
          </g>

          <path class="av-body" d={BODY_PATH} fill={`url(#${gradId})`} stroke="#3A3530" stroke-opacity="0.18" stroke-width="1.5" />

          <g class="av-face">
            <circle cx={CX - 21.5} cy="75.5" r="4.6" fill="#F2B8A0" opacity="0.45" />
            <circle cx={CX + 21.5} cy="75.5" r="4.6" fill="#F2B8A0" opacity="0.45" />
            <g class="av-look" style={{ transform: `translate(${gaze.x}px, ${gaze.y}px)` }}>
              <Eyes mood={mood} />
            </g>
            <Mouth mood={mood} />
          </g>
        </g>
      </g>

      {mood === 'done' && <Sparkles />}
      {mood === 'sleep' && <Zzz />}
    </svg>
  );
}
