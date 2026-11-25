// Tauri 셸과의 약속(선택, §12.1). 브라우저에서는 아무 일도 하지 않는다.
//   set_hit_rect {x, y, width, height}  — 클릭을 받을 영역(CSS px). 그 밖은 바탕화면으로 통과.
//   set_expanded {expanded}              — 패널이 펼쳐졌는지.
//   renew_session                        — 세션 쿠키가 만료됐다(401). 셸이 새 부트 코드로 이 창을 다시 연다.

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

/** 셸에 새 열쇠를 청하는 최소 간격. 셸도 30초에 한 번까지만 다시 연다(§12.1). */
export const RENEW_MIN_INTERVAL_MS = 30_000;
const RENEW_KEY = 'gyeot-renew-at';
let lastRenewAt: number | null = null;

function readRenewAt(): number | null {
  if (lastRenewAt !== null) return lastRenewAt;
  try {
    const v = Number(sessionStorage.getItem(RENEW_KEY));
    return Number.isFinite(v) && v > 0 ? v : null;
  } catch {
    return null;
  }
}

/**
 * 세션 쿠키가 만료됐을 때(401) 셸에 새 열쇠를 청한다. 셸 밖이거나 30초가 안 됐으면 부르지 않고 false.
 * 마지막으로 청한 시각은 이 창의 sessionStorage 에도 둔다 — 셸이 창을 다시 연 뒤 곧바로 또 401 이 나도
 * 다시 열기를 되풀이하지 않게.
 */
export function renewSession(now: number = Date.now()): boolean {
  const invoke = getInvoke();
  if (!invoke) return false;
  const last = readRenewAt();
  if (last !== null && now >= last && now - last < RENEW_MIN_INTERVAL_MS) return false;
  lastRenewAt = now;
  try {
    sessionStorage.setItem(RENEW_KEY, String(now));
  } catch {
    /* 저장이 막혀도 이 창의 메모리로는 막는다 */
  }
  try {
    void invoke('renew_session').catch(() => {});
  } catch {
    /* 셸이 이 명령을 모르면 조용히 넘어간다 */
  }
  return true;
}

/** 시험용: 기억해 둔 마지막 값을 지운다(sessionStorage 는 그대로 — 새로 고침 흉내). */
export function resetTauriMemo(): void {
  lastRect = '';
  lastExpanded = null;
  lastRenewAt = null;
}
