// contracts §1: 데이터 폴더·토큰·데몬 주소 해석.

import os from 'node:os';
import path from 'node:path';

export const DEFAULT_URL = 'http://127.0.0.1:47321';

export interface Env {
  readonly [key: string]: string | undefined;
}

/**
 * 데이터 폴더. `GYEOT_HOME`이 있으면 그것,
 * Windows는 `%LOCALAPPDATA%\gyeot`, 그 밖은 `$XDG_STATE_HOME/gyeot` 또는 `~/.local/state/gyeot`.
 */
export function resolveDataDir(
  env: Env,
  platform: NodeJS.Platform = process.platform,
  homedir: string = os.homedir(),
): string {
  const p = platform === 'win32' ? path.win32 : path.posix;
  const home = nonEmpty(env['GYEOT_HOME']);
  if (home) return p.resolve(home);
  if (platform === 'win32') {
    const local = nonEmpty(env['LOCALAPPDATA']) ?? p.join(homedir, 'AppData', 'Local');
    return p.join(local, 'gyeot');
  }
  // XDG 규격: 상대 경로는 무시한다.
  const xdg = nonEmpty(env['XDG_STATE_HOME']);
  if (xdg && p.isAbsolute(xdg)) return p.join(xdg, 'gyeot');
  return p.join(homedir, '.local', 'state', 'gyeot');
}

export function tokenPath(dataDir: string, platform: NodeJS.Platform = process.platform): string {
  return (platform === 'win32' ? path.win32 : path.posix).join(dataDir, 'token');
}

export type UrlResult = { ok: true; origin: string } | { ok: false; error: string };

/**
 * 데몬 주소. 토큰을 실어 보내므로 이 컴퓨터의 루프백(http://127.0.0.1, http://localhost)만 받는다.
 * 데몬은 127.0.0.1에만 묶인다(contracts §7).
 */
export function resolveDaemonUrl(env: Env): UrlResult {
  const raw = nonEmpty(env['GYEOT_URL']) ?? DEFAULT_URL;
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return { ok: false, error: `GYEOT_URL이 주소 꼴이 아닙니다(${raw})` };
  }
  if (url.protocol !== 'http:' || (url.hostname !== '127.0.0.1' && url.hostname !== 'localhost')) {
    return {
      ok: false,
      error: `GYEOT_URL은 http://127.0.0.1:<포트> 또는 http://localhost:<포트>만 됩니다(${url.origin})`,
    };
  }
  if (url.username || url.password) {
    return { ok: false, error: 'GYEOT_URL에 사용자 정보를 넣을 수 없습니다' };
  }
  return { ok: true, origin: url.origin };
}

/** 시험용: 재연결 대기의 기본 단위(ms). 기본 1000. */
export function resolveBackoffUnitMs(env: Env): number {
  const raw = nonEmpty(env['GYEOT_CHANNEL_BACKOFF_MS']);
  if (!raw) return 1000;
  const n = Number(raw);
  return Number.isInteger(n) && n >= 10 && n <= 60_000 ? n : 1000;
}

function nonEmpty(v: string | undefined): string | undefined {
  const t = v?.trim();
  return t ? t : undefined;
}
