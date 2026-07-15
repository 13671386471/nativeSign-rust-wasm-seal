#!/usr/bin/env python3
"""
生成一个符合 GB/T 33190-2016 标准的 OFD 测试文件。
包含：文本、路径（矩形/圆角/圆形/线条）、图片、印章占位等多种元素。
"""

import zipfile
import io
import os
from PIL import Image

NS = "http://www.ofdspec.org/2016"

def make_ofd_xml() -> str:
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<ofd:OFD xmlns:ofd="{NS}" Version="1.0" DocType="Ofd">
  <ofd:DocBody>
    <ofd:DocInfo>
      <ofd:DocID>TEST_OFD_2026_001</ofd:DocID>
      <ofd:Title>OFD 渲染测试文档</ofd:Title>
      <ofd:Author>rust-wasm-seal</ofd:Author>
      <ofd:CreationDate>2026-07-14T10:00:00</ofd:CreationDate>
    </ofd:DocInfo>
    <ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>
  </ofd:DocBody>
</ofd:OFD>"""

def make_document_xml() -> str:
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="{NS}">
  <ofd:CommonData>
    <ofd:MaxUnitID>50</ofd:MaxUnitID>
    <ofd:PageArea>
      <ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox>
    </ofd:PageArea>
    <ofd:PublicRes>DocumentRes.xml</ofd:PublicRes>
  </ofd:CommonData>
  <ofd:Pages>
    <ofd:Page ID="1" BaseLoc="Pages/Page_1.xml"/>
    <ofd:Page ID="2" BaseLoc="Pages/Page_2.xml"/>
  </ofd:Pages>
</ofd:Document>"""

def make_document_res_xml() -> str:
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<ofd:Res xmlns:ofd="{NS}" BaseLoc="Res">
  <ofd:Fonts>
    <ofd:Font ID="F1" FamilyName="SimSun"/>
    <ofd:Font ID="F2" FamilyName="SimHei"/>
    <ofd:Font ID="F3" FamilyName="KaiTi"/>
  </ofd:Fonts>
  <ofd:MultiMedias>
    <ofd:MultiMedia ID="M1" Type="Image">
      <ofd:MediaFile>test_image.png</ofd:MediaFile>
    </ofd:MultiMedia>
  </ofd:MultiMedias>
</ofd:Res>"""

def make_page_1_xml() -> str:
    """第一页：综合测试页 — 文本、路径、图片"""
    objects = []

    # ========== 标题 ==========
    objects.append(text_obj("T1", 20, 25, "OFD 渲染测试文档", font="F2", size=8.5, color="156 0 6"))

    # ========== 副标题 ==========
    objects.append(text_obj("T2", 20, 40, "文本渲染测试 — 不同字体与颜色", font="F2", size=5.0, color="0 0 0"))

    # ========== 正文（宋体） ==========
    lines = [
        "第一条 本测试文档用于验证 OFD 真实渲染功能。",
        "第二条 支持文本、路径、图片等多种 OFD 对象的渲染。",
        "第三条 文本渲染支持 SimSun、SimHei、KaiTi 等中文字体。",
        "第四条 路径渲染支持矩形、圆角矩形、圆形、椭圆弧等图形。",
        "第五条 图片渲染支持通过 ResourceID 引用公共资源中的 PNG 图像。",
        "第六条 所有渲染均在浏览器本地完成，无需服务端参与。",
    ]
    y = 55
    for i, line in enumerate(lines):
        color = "0 0 0" if i % 2 == 0 else "80 80 80"
        objects.append(text_obj(f"T3_{i}", 20, y, line, font="F1", size=3.8, color=color))
        y += 7.5

    # ========== 楷体段落 ==========
    objects.append(text_obj("T4", 20, y + 5, "（本段使用楷体字体测试）渲染引擎应正确映射字体名称。", font="F3", size=3.8, color="0 0 139"))

    # ========== 路径测试标题 ==========
    y2 = y + 22
    objects.append(text_obj("T5", 20, y2, "路径渲染测试 — 矩形、圆角、圆形、线条", font="F2", size=5.0, color="0 0 0"))

    # ========== 矩形（填充） ==========
    objects.append(path_rect("P1", 20, y2 + 12, 35, 20, fill="255 200 200", stroke="200 0 0", line_width=0.5))
    objects.append(text_obj("T6", 22, y2 + 24, "填充矩形", font="F1", size=3.0, color="139 0 0"))

    # ========== 圆角矩形（描边） ==========
    objects.append(path_round_rect("P2", 65, y2 + 12, 35, 20, r=3, fill="200 230 255", stroke="0 100 200", line_width=0.8))
    objects.append(text_obj("T7", 67, y2 + 24, "圆角矩形", font="F1", size=3.0, color="0 50 150"))

    # ========== 圆形（填充+描边） ==========
    objects.append(path_circle("P3", 125, y2 + 22, 10, fill="255 255 200", stroke="200 150 0", line_width=0.6))
    objects.append(text_obj("T8", 118, y2 + 24, "圆形", font="F1", size=3.0, color="139 100 0"))

    # ========== 椭圆弧（四分之一圆弧） ==========
    objects.append(path_arc("P4", 155, y2 + 22, 10, fill="230 255 230", stroke="0 150 0", line_width=0.6))
    objects.append(text_obj("T9", 150, y2 + 24, "圆弧", font="F1", size=3.0, color="0 100 0"))

    # ========== 线条 ==========
    objects.append(path_line("P5", 20, y2 + 42, 180, y2 + 42, stroke="0 0 0", line_width=0.3))
    objects.append(text_obj("T10", 85, y2 + 46, "水平分割线", font="F1", size=2.8, color="100 100 100"))

    # ========== 图片测试 ==========
    y3 = y2 + 55
    objects.append(text_obj("T11", 20, y3, "图片渲染测试 — 引用公共资源中的 PNG 图像", font="F2", size=5.0, color="0 0 0"))
    objects.append(image_obj("I1", "M1", 20, y3 + 10, 40, 30))
    objects.append(text_obj("T12", 70, y3 + 22, "左侧为内嵌 PNG 图片（红色方块）", font="F1", size=3.5, color="0 0 0"))

    # ========== 页面底部信息 ==========
    objects.append(text_obj("T13", 20, 280, "第 1 页 / 共 2 页  —  OFD 真实渲染测试", font="F1", size=3.0, color="150 150 150"))

    return f"""<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="{NS}" ID="1">
  <ofd:Area>
    <ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox>
  </ofd:Area>
  <ofd:Content>
{chr(10).join(objects)}
  </ofd:Content>
</ofd:Page>"""

def make_page_2_xml() -> str:
    """第二页：更多图形测试 — 复杂路径、渐变效果模拟"""
    objects = []

    objects.append(text_obj("P2_T1", 20, 25, "第二页 — 复杂路径与多元素测试", font="F2", size=7.0, color="0 0 0"))

    # 多行不同颜色的文本
    colors = [("200 0 0", "红色文本"), ("0 150 0", "绿色文本"), ("0 0 200", "蓝色文本"), ("200 100 0", "橙色文本")]
    y = 45
    for i, (c, label) in enumerate(colors):
        objects.append(text_obj(f"P2_T{i+2}", 20, y, label, font="F1", size=4.5, color=c))
        y += 10

    # 三角形（多边形路径）
    objects.append(path_triangle("P2_P1", 90, 45, 30, fill="255 220 220", stroke="200 0 0", line_width=0.5))

    # 星形（复杂路径）
    objects.append(path_star("P2_P2", 150, 65, 15, fill="255 255 200", stroke="200 150 0", line_width=0.5))

    # 嵌套矩形框
    objects.append(path_rect("P2_P3", 20, 100, 170, 60, fill="245 245 245", stroke="150 150 150", line_width=0.3))
    objects.append(path_rect("P2_P4", 30, 110, 150, 40, fill="255 255 255", stroke="0 0 0", line_width=0.5))
    objects.append(text_obj("P2_T6", 35, 132, "嵌套矩形框测试 — 内外边框", font="F1", size=3.5, color="0 0 0"))

    # 底部说明
    objects.append(text_obj("P2_T7", 20, 280, "第 2 页 / 共 2 页  —  渲染测试结束", font="F1", size=3.0, color="150 150 150"))

    return f"""<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="{NS}" ID="2">
  <ofd:Area>
    <ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox>
  </ofd:Area>
  <ofd:Content>
{chr(10).join(objects)}
  </ofd:Content>
</ofd:Page>"""

# ======================== 辅助函数 ========================

def text_obj(oid: str, x: float, y: float, text: str, font: str, size: float, color: str) -> str:
    """生成 TextObject XML"""
    return f"""    <ofd:TextObject ID="{oid}" CTM="1 0 0 1 0 0" Font="{font}" Size="{size}" FillColor="{color}">
      <ofd:TextCode X="{x}" Y="{y}">{escape_xml(text)}</ofd:TextCode>
    </ofd:TextObject>"""

def image_obj(oid: str, resource_id: str, x: float, y: float, w: float, h: float) -> str:
    """生成 ImageObject XML"""
    return f"""    <ofd:ImageObject ID="{oid}" CTM="1 0 0 1 0 0" ResourceID="{resource_id}" Boundary="{x} {y} {w} {h}"/>"""

def path_rect(oid: str, x: float, y: float, w: float, h: float, fill: str, stroke: str, line_width: float) -> str:
    """矩形路径"""
    d = f"M {x} {y} L {x+w} {y} L {x+w} {y+h} L {x} {y+h} Z"
    return path_obj(oid, d, fill, stroke, line_width)

def path_round_rect(oid: str, x: float, y: float, w: float, h: float, r: float, fill: str, stroke: str, line_width: float) -> str:
    """圆角矩形路径 — 使用二次贝塞尔曲线 Q"""
    d = (f"M {x+r} {y} L {x+w-r} {y} Q {x+w} {y} {x+w} {y+r} "
         f"L {x+w} {y+h-r} Q {x+w} {y+h} {x+w-r} {y+h} "
         f"L {x+r} {y+h} Q {x} {y+h} {x} {y+h-r} "
         f"L {x} {y+r} Q {x} {y} {x+r} {y} Z")
    return path_obj(oid, d, fill, stroke, line_width)

def path_circle(oid: str, cx: float, cy: float, r: float, fill: str, stroke: str, line_width: float) -> str:
    """圆形路径 — 使用 A (arc) 命令画两个半圆"""
    d = (f"M {cx-r} {cy} A {r} {r} 0 0 1 {cx+r} {cy} "
         f"A {r} {r} 0 0 1 {cx-r} {cy} Z")
    return path_obj(oid, d, fill, stroke, line_width)

def path_arc(oid: str, cx: float, cy: float, r: float, fill: str, stroke: str, line_width: float) -> str:
    """四分之一圆弧 + 闭合形成扇形"""
    d = (f"M {cx} {cy} L {cx+r} {cy} A {r} {r} 0 0 1 {cx} {cy+r} Z")
    return path_obj(oid, d, fill, stroke, line_width)

def path_line(oid: str, x1: float, y1: float, x2: float, y2: float, stroke: str, line_width: float) -> str:
    """直线路径（仅描边，无填充）"""
    d = f"M {x1} {y1} L {x2} {y2}"
    return path_obj(oid, d, fill=None, stroke=stroke, line_width=line_width)

def path_triangle(oid: str, x: float, y: float, size: float, fill: str, stroke: str, line_width: float) -> str:
    """三角形"""
    d = f"M {x} {y} L {x+size} {y} L {x+size/2} {y-size*0.866} Z"
    return path_obj(oid, d, fill, stroke, line_width)

def path_star(oid: str, cx: float, cy: float, r: float, fill: str, stroke: str, line_width: float) -> str:
    """五角星路径"""
    import math
    pts = []
    for i in range(10):
        angle = math.radians(-90 + i * 36)
        radius = r if i % 2 == 0 else r * 0.4
        px = cx + radius * math.cos(angle)
        py = cy + radius * math.sin(angle)
        pts.append(f"{px:.2f} {py:.2f}")
    d = f"M {pts[0]} L " + " L ".join(pts[1:]) + " Z"
    return path_obj(oid, d, fill, stroke, line_width)

def path_obj(oid: str, d: str, fill: str | None, stroke: str, line_width: float) -> str:
    """通用 PathObject XML"""
    fill_attr = f' FillColor="{fill}"' if fill else ''
    return f"""    <ofd:PathObject ID="{oid}" CTM="1 0 0 1 0 0"{fill_attr} StrokeColor="{stroke}" LineWidth="{line_width}">
      <ofd:AbbreviatedData>{d}</ofd:AbbreviatedData>
    </ofd:PathObject>"""

def escape_xml(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

def make_test_png() -> bytes:
    """生成一个红色方块的测试 PNG 图片"""
    img = Image.new("RGBA", (200, 150), (255, 0, 0, 255))
    # 在中心画一个白色圆
    from PIL import ImageDraw
    draw = ImageDraw.Draw(img)
    draw.ellipse([50, 25, 150, 125], fill=(255, 255, 255, 255))
    draw.rectangle([80, 55, 120, 95], fill=(0, 0, 255, 255))
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()

def generate_ofd(output_path: str):
    """生成 OFD 测试文件"""
    with zipfile.ZipFile(output_path, 'w', zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("OFD.xml", make_ofd_xml())
        zf.writestr("Doc_0/Document.xml", make_document_xml())
        zf.writestr("Doc_0/DocumentRes.xml", make_document_res_xml())
        zf.writestr("Doc_0/Pages/Page_1.xml", make_page_1_xml())
        zf.writestr("Doc_0/Pages/Page_2.xml", make_page_2_xml())
        zf.writestr("Doc_0/Res/test_image.png", make_test_png())

    print(f"[OK] 已生成 OFD 测试文件: {output_path}")
    # 验证 ZIP 内容
    with zipfile.ZipFile(output_path, 'r') as zf:
        print("[INFO] ZIP 内容列表:")
        for name in zf.namelist():
            info = zf.getinfo(name)
            print(f"       {name:40s}  {info.file_size:>8,} bytes")

if __name__ == "__main__":
    out = os.path.join(os.path.dirname(__file__), "..", "test_ofd_render.ofd")
    generate_ofd(os.path.abspath(out))
