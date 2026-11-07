#!/usr/bin/env python3
"""从矢量母版生成全套图标。

母版（手工维护，改图标只改这里）：
  src-tauri/icons/source/char.svg      角色
  src-tauri/icons/source/wordmark.svg  字标

产物：
  src-tauri/icons/icon.png             App 图标 1024（白色圆角底 + 角色）
  src-tauri/icons/tray/{16,32}.png     Windows 托盘（彩色）
  src-tauri/icons/tray-template/*.png  macOS 托盘（模板图：身体实心、卡片半透明、五官镂空）
  public/logo.svg                      设置页、关于页（favicon 同源）

依赖：无头 Chrome（渲染 SVG）、`npx tauri icon`（生成 icns/ico 及各平台尺寸）
"""
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src-tauri" / "icons" / "source"
ICONS = ROOT / "src-tauri" / "icons"
PUBLIC = ROOT / "public"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

PURPLE = "#8861F2"
CARD = "#FFFFFF"


def inner(svg: str) -> str:
    """取出 <svg> 里的内容，丢掉外层标签"""
    body = re.sub(r"^.*?<svg[^>]*>", "", svg, flags=re.S)
    return body.replace("</svg>", "").strip()


def bbox_of(path: Path, size: int) -> tuple[int, int, int, int]:
    """渲染一次，量出实际绘制区域，用来裁掉多余留白"""
    from PIL import Image
    import numpy as np

    tmp = SRC / "_bbox.png"
    render(path, size, tmp)
    a = np.array(Image.open(tmp).convert("RGBA"))[:, :, 3]
    ys, xs = np.where(a > 12)
    tmp.unlink(missing_ok=True)
    return int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max())


def render(svg_path: Path, px: int, dest: Path) -> None:
    subprocess.run(
        [CHROME, "--headless=new", "--disable-gpu", "--hide-scrollbars",
         "--default-background-color=00000000", "--force-device-scale-factor=1",
         f"--window-size={px},{px}", "--virtual-time-budget=2000",
         f"--screenshot={dest}", f"file://{svg_path}"],
        capture_output=True, check=True,
    )


def place(content: str, box_w: float, box_h: float, size: int, pad: float = 0.06) -> str:
    """把内容等比缩放并居中到 size×size 画布里"""
    k = (size * (1 - 2 * pad)) / max(box_w, box_h)
    tx = (size - box_w * k) / 2
    ty = (size - box_h * k) / 2
    return f'<g transform="translate({tx:.2f},{ty:.2f}) scale({k:.5f})">{content}</g>'


def main() -> None:
    char = inner((SRC / "char.svg").read_text())
    word = inner((SRC / "wordmark.svg").read_text())

    # 母版画布尺寸取自 svg 的 viewBox
    def vb(name: str) -> tuple[float, float]:
        m = re.search(r'viewBox="0 0 ([\d.]+) ([\d.]+)"', (SRC / name).read_text())
        return float(m.group(1)), float(m.group(2))

    cw, ch = vb("char.svg")
    ww, wh = vb("wordmark.svg")

    # 1) logo.svg：角色本身
    (PUBLIC / "logo.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {cw} {ch}" width="{cw}" height="{ch}">{char}</svg>'
    )

    # 2) appicon.svg：白色圆角底 + 角色
    size = 1024
    r = size * 0.2237
    (SRC / "appicon.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" width="{size}" height="{size}">'
        f'<rect width="{size}" height="{size}" rx="{r:.0f}" fill="{CARD}"/>'
        f'{place(char, cw, ch, size, pad=0.11)}</svg>'
    )

    # 3) 托盘：彩色 + 模板（中间产物，构建后即弃）
    mono_map = {
        PURPLE: "#000000", "#7A51E8": "#000000",
        CARD: "rgba(0,0,0,0.42)",
        "#C9BEF8": "rgba(0,0,0,0)", "#2A2340": "rgba(0,0,0,0)",
    }
    mono = re.sub(r'fill="(#[0-9A-Fa-f]{6})"',
                  lambda m: f'fill="{mono_map.get(m.group(1).upper(), "#000000")}"', char)

    for px in (16, 32):
        for name, body in (("tray", char), ("tray-template", mono)):
            (SRC / f"{name}-{px}.svg").write_text(
                f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {px} {px}" width="{px}" height="{px}">'
                f'{place(body, cw, ch, px, pad=0.02)}</svg>'
            )

    # 4) 渲染 PNG
    render(SRC / "appicon.svg", size, ICONS / "icon.png")
    for px in (16, 32):
        render(SRC / f"tray-{px}.svg", px, ICONS / "tray" / f"{px}x{px}.png")
        render(SRC / f"tray-template-{px}.svg", px, ICONS / "tray-template" / f"{px}x{px}.png")

    # 5) 由 1024 源图生成 icns / ico / 各平台尺寸
    subprocess.run(["npx", "tauri", "icon", str(ICONS / "icon.png"), "-o", str(ICONS)],
                   cwd=ROOT, check=True, capture_output=True)
    # tauri icon 会把 icon.png 压到 512，最后再补一次 1024 的源图
    render(SRC / "appicon.svg", size, ICONS / "icon.png")

    # 6) 清掉 tauri icon 生成的移动端目录（桌面项目用不到）
    for d in ("android", "ios"):
        shutil.rmtree(ICONS / d, ignore_errors=True)

    # 字标留给以后做横幅用
    render(SRC / "wordmark.svg", 1024, PUBLIC / "wordmark.png") if False else None

    print("icons rebuilt")


if __name__ == "__main__":
    main()
