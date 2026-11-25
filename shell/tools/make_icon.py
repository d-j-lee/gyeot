#!/usr/bin/env python3
"""곁 앱 아이콘 원본(1024×1024, 투명 배경)을 그린다.

docs/avatar-spec.md 의 `idle` 모습을 그대로 따른다.
- 몸: 초타원(n≈2.6), 가로 100 : 세로 88, 미색 #F4EFE6 → 아래 30% #E6DDCF 그라데이션,
  외곽선 #3A3530 18% (표시 크기 128px 기준 1.5px)
- 바닥 그림자: #000 10% 타원
- 눈: 세로로 긴 타원 #2B2622, 가운데보다 약간 위, 간격은 몸 너비의 28%, 흰 하이라이트 점
- 볼: #F2B8A0 45%, 눈 바깥 아래의 작은 원
- 입: idle 에는 없다
- 새싹: 머리 오른쪽 위, 줄기 하나 + 잎 하나, 잎은 세이지 #9DB89A

사용:  python3 shell/tools/make_icon.py [출력.png]
그 뒤: cd shell && npx --yes @tauri-apps/cli@2 icon assets/gyeot-idle-1024.png -o src-tauri/icons
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

SIZE = 1024
SS = 4  # 초표본(4배로 그린 뒤 줄여서 가장자리를 부드럽게)
S = SIZE * SS

BODY = (0xF4, 0xEF, 0xE6)
BODY_SHADE = (0xE6, 0xDD, 0xCF)
OUTLINE = (0x3A, 0x35, 0x30)
EYE = (0x2B, 0x26, 0x22)
CHEEK = (0xF2, 0xB8, 0xA0)
LEAF = (0x9D, 0xB8, 0x9A)
STEM = (0x7F, 0x9C, 0x7C)


def px(v: float) -> float:
    """1024 기준 좌표를 초표본 캔버스 좌표로."""
    return v * SS


def superellipse(cx: float, cy: float, a: float, b: float, n: float, steps: int = 720):
    pts = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        x = a * math.copysign(abs(c) ** (2 / n), c)
        y = b * math.copysign(abs(s) ** (2 / n), s)
        pts.append((cx + x, cy + y))
    return pts


def layer() -> Image.Image:
    return Image.new("RGBA", (S, S), (0, 0, 0, 0))


def with_alpha(img: Image.Image, opacity: float) -> Image.Image:
    r, g, b, a = img.split()
    a = a.point(lambda v: int(round(v * opacity)))
    return Image.merge("RGBA", (r, g, b, a))


def quad_bezier(p0, p1, p2, steps: int = 64):
    out = []
    for i in range(steps + 1):
        t = i / steps
        x = (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t**2 * p2[0]
        y = (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t**2 * p2[1]
        out.append((x, y))
    return out


def leaf_shape(base, tip, bulge: float):
    """밑동에서 끝까지 양쪽으로 부푼 잎(아몬드꼴) 다각형."""
    bx, by = base
    tx, ty = tip
    mx, my = (bx + tx) / 2, (by + ty) / 2
    dx, dy = tx - bx, ty - by
    length = math.hypot(dx, dy)
    nx, ny = -dy / length, dx / length  # 법선
    c1 = (mx + nx * bulge, my + ny * bulge)
    c2 = (mx - nx * bulge * 0.8, my - ny * bulge * 0.8)
    side1 = quad_bezier(base, c1, tip)
    side2 = quad_bezier(tip, c2, base)
    return side1 + side2[1:]


def draw(out_path: Path) -> None:
    canvas = layer()

    # 몸 크기: 가로 100 : 세로 88
    cx, cy = 512.0, 572.0
    a = 372.0
    b = a * 0.88
    top, bottom = cy - b, cy + b

    # 바닥 그림자
    shadow = layer()
    ImageDraw.Draw(shadow).ellipse(
        [px(cx - 290), px(bottom + 6), px(cx + 290), px(bottom + 74)], fill=(0, 0, 0, 255)
    )
    canvas = Image.alpha_composite(canvas, with_alpha(shadow, 0.10))

    # 새싹(몸 뒤에서 솟아 나오도록 몸보다 먼저 그린다)
    sprout = layer()
    sd = ImageDraw.Draw(sprout)
    stem_base = (cx + 120, top + 40)
    stem_tip = (cx + 168, top - 92)
    stem = quad_bezier(stem_base, (cx + 112, top - 40), stem_tip)
    sd.line([(px(x), px(y)) for x, y in stem], fill=STEM + (255,), width=int(px(22)), joint="curve")
    r = px(11)
    sd.ellipse([px(stem_tip[0]) - r, px(stem_tip[1]) - r, px(stem_tip[0]) + r, px(stem_tip[1]) + r], fill=STEM + (255,))
    leaf = leaf_shape(stem_tip, (cx + 330, top - 168), bulge=-62)
    sd.polygon([(px(x), px(y)) for x, y in leaf], fill=LEAF + (255,))
    # 잎맥
    vein = quad_bezier(stem_tip, (cx + 250, top - 118), (cx + 312, top - 160), steps=32)
    sd.line([(px(x), px(y)) for x, y in vein], fill=STEM + (255,), width=int(px(7)), joint="curve")
    canvas = Image.alpha_composite(canvas, sprout)

    # 몸: 마스크 + 세로 그라데이션(아래 30%)
    body_pts = [(px(x), px(y)) for x, y in superellipse(cx, cy, a, b, 2.6)]
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).polygon(body_pts, fill=255)
    grad = Image.new("RGBA", (S, S), BODY + (255,))
    gd = ImageDraw.Draw(grad)
    shade_start = top + (bottom - top) * 0.70
    y0, y1 = int(px(shade_start)), int(px(bottom))
    for y in range(y0, y1 + 1):
        t = (y - y0) / max(1, (y1 - y0))
        col = tuple(int(round(BODY[i] + (BODY_SHADE[i] - BODY[i]) * t)) for i in range(3))
        gd.line([(0, y), (S, y)], fill=col + (255,))
    body = layer()
    body.paste(grad, (0, 0), mask)
    canvas = Image.alpha_composite(canvas, body)

    # 외곽선: 128px 표시 기준 1.5px → 1024 에서 12px
    outline = layer()
    ImageDraw.Draw(outline).line(body_pts + [body_pts[0]], fill=OUTLINE + (255,), width=int(px(12)), joint="curve")
    canvas = Image.alpha_composite(canvas, with_alpha(outline, 0.18))

    # 볼(눈 바깥 아래)
    body_w = 2 * a
    eye_dx = body_w * 0.28 / 2
    eye_y = cy - 46
    cheeks = layer()
    cd = ImageDraw.Draw(cheeks)
    for sgn in (-1, 1):
        ccx = cx + sgn * (eye_dx + 64)
        ccy = eye_y + 82
        cr = 40
        cd.ellipse([px(ccx - cr), px(ccy - cr), px(ccx + cr), px(ccy + cr)], fill=CHEEK + (255,))
    canvas = Image.alpha_composite(canvas, with_alpha(cheeks, 0.45))

    # 눈 + 하이라이트
    eyes = layer()
    ed = ImageDraw.Draw(eyes)
    erx, ery = 30, 46
    for sgn in (-1, 1):
        ex = cx + sgn * eye_dx
        ed.ellipse([px(ex - erx), px(eye_y - ery), px(ex + erx), px(eye_y + ery)], fill=EYE + (255,))
        hr = 11
        hx, hy = ex - 9, eye_y - 18
        ed.ellipse([px(hx - hr), px(hy - hr), px(hx + hr), px(hy + hr)], fill=(255, 255, 255, 255))
    canvas = Image.alpha_composite(canvas, eyes)

    # 몸 밖으로 나간 그림자·새싹은 그대로 두고, 완전히 투명한 곳은 0으로 정리
    out = canvas.resize((SIZE, SIZE), Image.LANCZOS)
    alpha = out.getchannel("A")
    cleaned = Image.new("RGBA", out.size, (0, 0, 0, 0))
    cleaned.paste(out, (0, 0), ImageChops.lighter(alpha, Image.new("L", out.size, 0)))
    out_path.parent.mkdir(parents=True, exist_ok=True)
    cleaned.save(out_path, "PNG", optimize=True)


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent / "assets" / "gyeot-idle-1024.png"
    draw(out)
    img = Image.open(out)
    assert img.size == (SIZE, SIZE) and img.mode == "RGBA", (img.size, img.mode)
    print(f"아이콘 원본을 썼어요: {out} ({img.size[0]}×{img.size[1]}, {img.mode})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
