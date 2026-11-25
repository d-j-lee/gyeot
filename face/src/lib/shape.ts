// 아바타 몸의 기하. Node 스크립트(앱 아이콘 만들기)도 같이 쓴다.

/** 초타원 |x/a|^n + |y/b|^n = 1 을 부드러운 닫힌 곡선(Catmull-Rom → 3차 베지에)으로. */
export function superellipsePath(cx: number, cy: number, a: number, b: number, n: number, segments = 48): string {
  const pts: [number, number][] = [];
  for (let i = 0; i < segments; i++) {
    const t = (i / segments) * Math.PI * 2;
    const c = Math.cos(t);
    const s = Math.sin(t);
    const x = cx + a * Math.sign(c) * Math.abs(c) ** (2 / n);
    const y = cy + b * Math.sign(s) * Math.abs(s) ** (2 / n);
    pts.push([x, y]);
  }
  const f = (v: number) => (Math.round(v * 100) / 100).toString();
  const p = (i: number) => pts[(i + segments) % segments]!;
  let d = `M${f(p(0)[0])} ${f(p(0)[1])}`;
  for (let i = 0; i < segments; i++) {
    const p0 = p(i - 1);
    const p1 = p(i);
    const p2 = p(i + 1);
    const p3 = p(i + 2);
    const c1x = p1[0] + (p2[0] - p0[0]) / 6;
    const c1y = p1[1] + (p2[1] - p0[1]) / 6;
    const c2x = p2[0] - (p3[0] - p1[0]) / 6;
    const c2y = p2[1] - (p3[1] - p1[1]) / 6;
    d += `C${f(c1x)} ${f(c1y)} ${f(c2x)} ${f(c2y)} ${f(p2[0])} ${f(p2[1])}`;
  }
  return `${d}Z`;
}
