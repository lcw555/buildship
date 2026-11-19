#!/usr/bin/env python3
"""生成 Buildship 应用图标（Aurora Loop）。

设计要点（方案 6.2）：
- 2~3 条连续流体极光丝带、开放式环流、中心自然负空间；
- 冰蓝 / 天蓝 / 蓝紫为主，少量青色与暖色高光；
- 品牌蓝渐变底，小尺寸仍可识别；
- 绿色在线状态点在运行时用 DOM 绘制，不烘焙进图标。

丝带路径与 `AuroraMark.vue` 保持一致（viewBox 0 0 64 64）。

用法：
    python scripts/generate-icon.py
    npx --prefix apps/desktop tauri icon apps/desktop/src-tauri/icons/source.png
"""

from __future__ import annotations

import os

from PIL import Image, ImageDraw, ImageFilter

SIZE = 1024
SS = 3  # 超采样倍数
W = SIZE * SS

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.path.join(ROOT, "apps", "desktop", "src-tauri", "icons")

# 每条丝带由若干段三次贝塞尔组成（坐标基于 64×64 viewBox）
RIB_R1 = [
    ((10, 44), (18, 24), (30, 14), (46, 16)),
    ((46, 16), (56, 17), (58, 28), (50, 34)),
    ((50, 34), (42, 40), (26, 38), (22, 48)),
    ((22, 48), (19, 55), (26, 58), (34, 54)),
]
RIB_R2 = [
    ((52, 20), (44, 40), (32, 50), (16, 48)),
    ((16, 48), (6, 47), (4, 36), (12, 30)),
    ((12, 30), (20, 24), (36, 26), (40, 16)),
    ((40, 16), (43, 9), (36, 6), (28, 10)),
]
RIB_R3 = [
    ((22, 14), (34, 18), (42, 26), (42, 36)),
    ((42, 36), (42, 46), (32, 52), (22, 48)),
]


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def lerp_color(c1, c2, t):
    return tuple(int(round(lerp(c1[i], c2[i], t))) for i in range(3))


def sample_path(segments, steps_per_seg=90):
    """把多段三次贝塞尔采样成密集折线（视口坐标）"""
    pts = []
    for seg in segments:
        p = [(v * W / 64.0) for point in seg for v in point]
        x0, y0, x1, y1, x2, y2, x3, y3 = p
        for i in range(steps_per_seg + 1):
            t = i / steps_per_seg
            mt = 1 - t
            x = mt**3 * x0 + 3 * mt**2 * t * x1 + 3 * mt * t**2 * x2 + t**3 * x3
            y = mt**3 * y0 + 3 * mt**2 * t * y1 + 3 * mt * t**2 * y2 + t**3 * y3
            pts.append((x, y))
    return pts


def stroke_mask(pts, width: float) -> Image.Image:
    """生成一条丝带的覆盖蒙版：密集折线 + 圆头收尾"""
    mask = Image.new("L", (W, W), 0)
    draw = ImageDraw.Draw(mask)
    draw.line(pts, fill=255, width=int(width))
    r = width / 2
    for point in (pts[0], pts[-1]):
        draw.ellipse([point[0] - r, point[1] - r, point[0] + r, point[1] + r], fill=255)
    return mask


def vertical_gradient(stops) -> Image.Image:
    """三色纵向渐变，用于给丝带着色"""
    ramp = Image.new("RGB", (1, 512))
    for y in range(512):
        t = y / 511
        if t < 0.5:
            color = lerp_color(stops[0], stops[1], t * 2)
        else:
            color = lerp_color(stops[1], stops[2], (t - 0.5) * 2)
        ramp.putpixel((0, y), color)
    return ramp.resize((W, W), Image.BILINEAR)


def build_background() -> tuple[Image.Image, Image.Image]:
    radius = int(0.235 * W)
    mask = Image.new("L", (W, W), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, W - 1, W - 1], radius=radius, fill=255)

    ramp = Image.new("RGB", (1, 512))
    for y in range(512):
        t = y / 511
        if t < 0.55:
            color = lerp_color((78, 126, 255), (47, 107, 255), t / 0.55)
        else:
            color = lerp_color((47, 107, 255), (28, 48, 176), (t - 0.55) / 0.45)
        ramp.putpixel((0, y), color)
    bg = ramp.resize((W, W), Image.BILINEAR).convert("RGBA")

    # 左上冷光高光
    glow = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse([-0.35 * W, -0.45 * W, 0.72 * W, 0.42 * W], fill=(170, 220, 255, 70))
    glow = glow.filter(ImageFilter.GaussianBlur(0.09 * W))
    bg = Image.alpha_composite(bg, glow)

    transparent = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    return Image.composite(bg, transparent, mask), mask


def main() -> None:
    bg, rounded_mask = build_background()

    ribbons = [
        {
            "segments": RIB_R1,
            "width": 5.4 * W / 64.0,
            "colors": [(140, 226, 255), (255, 255, 255), (188, 208, 255)],
            "glow_alpha": 160,
        },
        {
            "segments": RIB_R2,
            "width": 4.6 * W / 64.0,
            "colors": [(178, 242, 255), (168, 204, 255), (168, 150, 255)],
            "glow_alpha": 140,
        },
        {
            "segments": RIB_R3,
            "width": 3.2 * W / 64.0,
            "colors": [(255, 224, 176), (196, 240, 255), (150, 200, 255)],
            "glow_alpha": 120,
        },
    ]

    art = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    glow = Image.new("RGBA", (W, W), (0, 0, 0, 0))

    for ribbon in ribbons:
        pts = sample_path(ribbon["segments"])
        mask = stroke_mask(pts, ribbon["width"])
        colored = vertical_gradient(ribbon["colors"]).convert("RGBA")
        layer = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        layer.paste(colored, (0, 0), mask)
        art = Image.alpha_composite(art, layer)

        glow_mask = stroke_mask(pts, ribbon["width"] * 2.4)
        glow_layer = Image.new("RGBA", (W, W), (0, 0, 0, 0))
        glow_layer.paste(colored, (0, 0), glow_mask)
        glow_layer.putalpha(glow_layer.getchannel("A").point(lambda v: int(v * ribbon["glow_alpha"] / 255)))
        glow = Image.alpha_composite(glow, glow_layer)

    glow = glow.filter(ImageFilter.GaussianBlur(int(0.022 * W)))
    icon = Image.alpha_composite(bg, glow)
    icon = Image.alpha_composite(icon, art)
    icon = Image.composite(icon, Image.new("RGBA", (W, W), (0, 0, 0, 0)), rounded_mask)

    icon = icon.resize((SIZE, SIZE), Image.LANCZOS)
    os.makedirs(OUT_DIR, exist_ok=True)
    target = os.path.join(OUT_DIR, "source.png")
    icon.save(target)
    print(f"已生成 {target} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()
