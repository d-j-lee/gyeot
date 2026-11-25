// Tauri 셸과의 약속(선택). 브라우저에서는 아무 일도 하지 않는다.
//   set_hit_rect {x, y, width, height}  — 클릭을 받을 영역(CSS px). 그 밖은 바탕화면으로 통과.
//   set_expanded {expanded}              — 패널이 펼쳐졌는지.

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

function getInvoke(): Invoke | null {
  const w = typeof window === 'undefined' ? undefined : (window as unknown as { __TAURI__?: { core?: { invoke?: unknown } } });
  const inv = w?.__TAURI__?.core?.invoke;
  return typeof inv === 'function' ? (inv as Invoke) : null;
}

export function inTauri(): boolean {
  return getInvoke() !== null;
}

async function call(cmd: string, args: Record<string, unknown>): Promise<void> {
  const invoke = getInvoke();
  if (!invoke) return;
  try {
    await invoke(cmd, args);
  } catch {
    /* 셸이 이 명령을 모르면 조용히 넘어간다 */
  }
}

export interface HitRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

let lastRect = '';
export function setHitRect(r: HitRect): void {
  const rect = {
    x: Math.max(0, Math.floor(r.x)),
    y: Math.max(0, Math.floor(r.y)),
    width: Math.ceil(r.width),
    height: Math.ceil(r.height),
  };
  const key = `${rect.x},${rect.y},${rect.width},${rect.height}`;
  if (key === lastRect) return;
  lastRect = key;
  void call('set_hit_rect', { ...rect });
}

let lastExpanded: boolean | null = null;
export function setExpanded(expanded: boolean): void {
  if (expanded === lastExpanded) return;
  lastExpanded = expanded;
  void call('set_expanded', { expanded });
}

/** 여러 요소를 감싸는 사각형(CSS px). 보이지 않는 요소는 뺀다. */
export function unionRect(els: readonly (Element | null | undefined)[], pad = 0): HitRect | null {
  let l = Infinity;
  let t = Infinity;
  let r = -Infinity;
  let b = -Infinity;
  for (const el of els) {
    if (!el) continue;
    const rc = el.getBoundingClientRect();
    if (rc.width === 0 && rc.height === 0) continue;
    l = Math.min(l, rc.left);
    t = Math.min(t, rc.top);
    r = Math.max(r, rc.right);
    b = Math.max(b, rc.bottom);
  }
  if (!Number.isFinite(l)) return null;
  return { x: l - pad, y: t - pad, width: r - l + pad * 2, height: b - t + pad * 2 };
}

/** 시험용: 기억해 둔 마지막 값을 지운다. */
export function resetTauriMemo(): void {
  lastRect = '';
  lastExpanded = null;
}
