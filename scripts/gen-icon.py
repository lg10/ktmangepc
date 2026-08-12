#!/usr/bin/env python3
"""从品牌图生成应用图标源图（1024x1024，macOS 圆角规范）。

- 画布 1024x1024 透明背景
- 以原图背景色铺底、原图等比 contain 居中
- 用 macOS 超椭圆近似圆角（半径 ≈ 22.4%）裁切，
  使 Dock 中视觉大小与系统图标一致且带圆角
"""
import sys
from PIL import Image, ImageDraw

SRC = sys.argv[1] if len(sys.argv) > 1 else "source.png"
OUT = sys.argv[2] if len(sys.argv) > 2 else "app-icon.png"
SIZE = 1024
# macOS 官方模板：1024 画布中圆角矩形为 832x832（四周留白 96px），
# 圆角半径约为形状边长的 22.4%（≈186px），与系统图标视觉大小一致
SHAPE = 832
RADIUS = int(SHAPE * 0.2237)
INSET = (SIZE - SHAPE) // 2

img = Image.open(SRC).convert("RGBA")

# 背景色取原图左上角像素（纯色底）
bg = img.getpixel((0, 0))

canvas = Image.new("RGBA", (SHAPE, SHAPE), bg)

# 等比 contain 居中铺满形状区域
scale = min(SHAPE / img.width, SHAPE / img.height)
w, h = int(img.width * scale), int(img.height * scale)
resized = img.resize((w, h), Image.LANCZOS)
canvas.paste(resized, ((SHAPE - w) // 2, (SHAPE - h) // 2), resized)

# 圆角遮罩
mask = Image.new("L", (SHAPE, SHAPE), 0)
draw = ImageDraw.Draw(mask)
draw.rounded_rectangle([0, 0, SHAPE - 1, SHAPE - 1], radius=RADIUS, fill=255)

out = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
out.paste(canvas, (INSET, INSET), mask)
out.save(OUT)
print(f"saved {OUT} {out.size}")
