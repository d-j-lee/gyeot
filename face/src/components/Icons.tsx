// 작은 선 아이콘(16px). 모두 장식이라 aria-hidden.
import type { ComponentChildren } from 'preact';

interface P {
  size?: number;
}

function Svg({ size = 16, children }: P & { children: ComponentChildren }) {
  return (
    <svg
      class="icon"
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      stroke-width="1.6"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

export const IconClose = (p: P) => (
  <Svg {...p}>
    <path d="M4 4l8 8M12 4l-8 8" />
  </Svg>
);

export const IconTerminal = (p: P) => (
  <Svg {...p}>
    <rect x="1.8" y="2.8" width="12.4" height="10.4" rx="2" />
    <path d="M4.6 6.4l2 1.6-2 1.6M8.4 10h3" />
  </Svg>
);

export const IconChevron = (p: P & { open?: boolean }) => (
  <Svg {...p}>
    <path d={p.open ? 'M4 10l4-4 4 4' : 'M4 6l4 4 4-4'} />
  </Svg>
);

export const IconSend = (p: P) => (
  <Svg {...p}>
    <path d="M2.5 8h9M8 3.5L12.5 8 8 12.5" />
  </Svg>
);

export const IconRefresh = (p: P) => (
  <Svg {...p}>
    <path d="M13 8a5 5 0 1 1-1.5-3.6M13 2.5v3h-3" />
  </Svg>
);

export const IconCheck = (p: P) => (
  <Svg {...p}>
    <path d="M3.2 8.4l3 3 6.6-6.8" />
  </Svg>
);

export const IconInfo = (p: P) => (
  <Svg {...p}>
    <circle cx="8" cy="8" r="6.2" />
    <path d="M8 7.2v4M8 4.9v.1" />
  </Svg>
);

export const IconWarn = (p: P) => (
  <Svg {...p}>
    <path d="M8 2.2l6.2 11H1.8z" />
    <path d="M8 6.4v3.2M8 11.4v.1" />
  </Svg>
);

export const IconCopy = (p: P) => (
  <Svg {...p}>
    <rect x="5.2" y="5.2" width="8.4" height="8.4" rx="1.6" />
    <path d="M10.6 3.4V3a1.2 1.2 0 0 0-1.2-1.2H3a1.2 1.2 0 0 0-1.2 1.2v6.4A1.2 1.2 0 0 0 3 10.6h.4" />
  </Svg>
);

export const IconMic = (p: P) => (
  <Svg {...p}>
    <rect x="5.6" y="1.8" width="4.8" height="8" rx="2.4" />
    <path d="M3.4 7.6a4.6 4.6 0 0 0 9.2 0M8 12.2v2" />
  </Svg>
);

export const IconClock = (p: P) => (
  <Svg {...p}>
    <circle cx="8" cy="8" r="6.2" />
    <path d="M8 4.6V8l2.4 1.4" />
  </Svg>
);
