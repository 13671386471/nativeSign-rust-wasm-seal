#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成包含文字、图片、图标等内容的 OFD 文件
支持嵌入位图图像（PNG/JPEG）和矢量图标
"""

import base64
import os
import struct
import uuid
import zipfile
import zlib
from datetime import datetime
from io import BytesIO

OUTPUT_DIR = os.path.dirname(os.path.abspath(__file__))
NS = "http://www.ofdspec.org/2016"

def xml_decl():
    return '<?xml version="1.0" encoding="UTF-8"?>'

# =====================================================================
# 图像生成工具
# =====================================================================

def create_simple_png(width, height, pixels_func):
    """
    创建 PNG 图像
    pixels_func: (x, y) -> (r, g, b, a) 返回每个像素的颜色
    """
    def create_png_data(width, height, pixels_func):
        raw_data = b''
        for y in range(height):
            raw_data += b'\x00'  # Filter type: None
            for x in range(width):
                r, g, b, a = pixels_func(x, y)
                raw_data += bytes([r, g, b, a])
        
        # Compress
        compressed = zlib.compress(raw_data)
        
        # Build PNG
        png = b'\x89PNG\r\n\x1a\n'
        
        # IHDR
        ihdr_data = struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)  # 8-bit RGBA
        ihdr_crc = zlib.crc32(b'IHDR' + ihdr_data)
        png += struct.pack('>I', 13) + b'IHDR' + ihdr_data + struct.pack('>I', ihdr_crc)
        
        # IDAT
        idat_crc = zlib.crc32(b'IDAT' + compressed)
        png += struct.pack('>I', len(compressed)) + b'IDAT' + compressed + struct.pack('>I', idat_crc)
        
        # IEND
        iend_crc = zlib.crc32(b'IEND')
        png += struct.pack('>I', 0) + b'IEND' + struct.pack('>I', iend_crc)
        
        return png
    
    return create_png_data(width, height, pixels_func)

def generate_test_image(width=100, height=100):
    """生成测试用渐变彩色图片"""
    def pixel_func(x, y):
        r = int(255 * x / width)
        g = int(255 * y / height)
        b = int(255 * (1 - x / width))
        a = 255
        return (r, g, b, a)
    return create_simple_png(width, height, pixel_func)

def generate_icon_image(icon_type="star", size=64):
    """生成图标图片"""
    def pixel_func(x, y):
        cx, cy = size // 2, size // 2
        dist = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
        
        if icon_type == "star":
            # 简单圆形图标
            if dist < size // 2 - 2:
                return (255, 200, 0, 255)  # 金色
            elif dist < size // 2:
                return (200, 150, 0, 255)  # 边框
            else:
                return (255, 255, 255, 0)  # 透明
        elif icon_type == "check":
            # 绿色对勾图标
            if dist < size // 2 - 2:
                return (46, 160, 67, 255)  # 绿色
            elif dist < size // 2:
                return (30, 120, 50, 255)  # 边框
            else:
                return (255, 255, 255, 0)
        elif icon_type == "warning":
            # 黄色警告图标
            if dist < size // 2 - 2:
                return (255, 193, 7, 255)  # 黄色
            elif dist < size // 2:
                return (200, 150, 0, 255)
            else:
                return (255, 255, 255, 0)
        else:
            return (200, 200, 200, 255)
    
    return create_simple_png(size, size, pixel_func)

def generate_seal_image(size=120):
    """生成印章图片（红色圆形）"""
    def pixel_func(x, y):
        cx, cy = size // 2, size // 2
        dist = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
        
        # 外圆环
        if size // 2 - 8 < dist < size // 2 - 2:
            return (220, 30, 30, 255)
        # 内圆环
        elif size // 3 - 3 < dist < size // 3 + 3:
            return (220, 30, 30, 255)
        # 中心五角星（简化）
        elif dist < 15:
            return (220, 30, 30, 255)
        else:
            return (255, 255, 255, 0)  # 透明
    
    return create_simple_png(size, size, pixel_func)

# =====================================================================
# OFD 结构构建
# =====================================================================

def build_ofd_xml():
    """构建 OFD.xml — 文档入口"""
    doc_id = uuid.uuid4().hex
    return f'''{xml_decl()}
<ofd:OFD xmlns:ofd="{NS}" DocType="OFD" Version="1.0">
  <ofd:DocBody>
    <ofd:DocInfo>
      <ofd:DocID>{doc_id}</ofd:DocID>
      <ofd:Title>图文混排示例文档</ofd:Title>
      <ofd:Author>OFD Image Demo Generator</ofd:Author>
      <ofd:Subject>包含文字、图片、图标的 OFD 示例</ofd:Subject>
      <ofd:CreationDate>{datetime.now().strftime('%Y-%m-%d')}</ofd:CreationDate>
      <ofd:Creator>Python OFD Generator</ofd:Creator>
    </ofd:DocInfo>
    <ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>
  </ofd:DocBody>
</ofd:OFD>'''

def build_document_xml(image_refs):
    """构建 Document.xml — 文档结构"""
    image_entries = []
    for img_id, img_info in image_refs.items():
        image_entries.append(f'        <ofd:MultiMedia ID="{img_id}" Type="Image">{img_info["path"]}</ofd:MultiMedia>')
    
    images_xml = '\n'.join(image_entries)
    
    return f'''{xml_decl()}
<ofd:Document xmlns:ofd="{NS}">
  <ofd:CommonData>
    <ofd:MaxUnitID>999</ofd:MaxUnitID>
    <ofd:PageArea>
      <ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox>
    </ofd:PageArea>
    <ofd:PublicRes>
      <ofd:Fonts>
        <ofd:Font ID="1" FontName="宋体" FamilyName="SimSun" Charset="GB2312"/>
        <ofd:Font ID="2" FontName="黑体" FamilyName="SimHei" Charset="GB2312"/>
        <ofd:Font ID="3" FontName="楷体" FamilyName="KaiTi" Charset="GB2312"/>
      </ofd:Fonts>
      <ofd:MultiMedias>
{images_xml}
      </ofd:MultiMedias>
    </ofd:PublicRes>
  </ofd:CommonData>
  <ofd:Pages>
    <ofd:Page ID="1" BaseLoc="Page_0/Content.xml"/>
    <ofd:Page ID="2" BaseLoc="Page_1/Content.xml"/>
    <ofd:Page ID="3" BaseLoc="Page_2/Content.xml"/>
  </ofd:Pages>
  <ofd:Annotations/>
</ofd:Document>'''

def build_page_content(page_num, image_refs):
    """构建页面内容"""
    lines = []
    
    if page_num == 0:
        # 第1页：封面 - 文字 + 大图片 + 图标
        build_cover_page(lines, image_refs)
    elif page_num == 1:
        # 第2页：图文混排 - 文字环绕图片
        build_mixed_content_page(lines, image_refs)
    elif page_num == 2:
        # 第3页：图标展示 - 各种图标 + 说明文字
        build_icons_page(lines, image_refs)
    
    content_str = '\n'.join(lines)
    return f'''{xml_decl()}
<ofd:Page xmlns:ofd="{NS}">
  <ofd:Content>
    <ofd:Layer ID="1" Type="Body" DrawParam="1">
{content_str}
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>'''

def build_cover_page(lines, image_refs):
    """构建封面页"""
    # 顶部装饰线
    lines.append('''    <ofd:PathObject ID="100" Boundary="25 15 160 0.5" LineWidth="0.5">
      <ofd:StrokeColor Value="0 102 204"/>
      <ofd:AbbreviatedData>M 25 15.2 L 185 15.2</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 大标题
    lines.append('''    <ofd:TextObject ID="101" Boundary="25 25 160 12" Font="2" Size="10">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="35" Y="35">OFD 图文混排示例</ofd:TextCode>
    </ofd:PathObject>''')
    
    # 副标题
    lines.append('''    <ofd:TextObject ID="102" Boundary="25 40 160 6" Font="3" Size="5">
      <ofd:FillColor Value="102 102 102"/>
      <ofd:TextCode X="45" Y="45">—— 支持文字、图片、图标混合排版 ——</ofd:TextCode>
    </ofd:PathObject>''')
    
    # 分隔线
    lines.append('''    <ofd:PathObject ID="103" Boundary="60 50 90 0.8" LineWidth="0.8">
      <ofd:StrokeColor Value="0 102 204"/>
      <ofd:AbbreviatedData>M 60 50.4 L 150 50.4</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 中央大图片
    img_id = image_refs.get("cover_image", {}).get("id", "1")
    lines.append(f'''    <ofd:ImageObject ID="200" Boundary="45 60 120 80" ImageRef="{img_id}" CTM="1 0 0 1 0 0">
      <ofd:FillColor Value="255 255 255"/>
    </ofd:ImageObject>''')
    
    # 图片说明
    lines.append('''    <ofd:TextObject ID="201" Boundary="55 145 100 5" Font="1" Size="4">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="70" Y="149">图1：渐变彩色示例图片</ofd:TextCode>
    </ofd:PathObject>''')
    
    # 文档信息
    info_items = [
        ("文档格式:", "OFD (GB/T 33190-2016)"),
        ("内容类型:", "文字 / 图片 / 图标"),
        ("生成工具:", "Python OFD Generator"),
        ("生成日期:", datetime.now().strftime('%Y年%m月%d日')),
    ]
    
    for i, (label, value) in enumerate(info_items):
        ly = 160 + i * 10
        lines.append(f'''    <ofd:TextObject ID="30{i}" Boundary="35 {ly} 30 5" Font="2" Size="4.5">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="35" Y="{ly+3.8}">{label}</ofd:TextCode>
    </ofd:TextObject>''')
        lines.append(f'''    <ofd:TextObject ID="31{i}" Boundary="65 {ly} 100 5" Font="3" Size="4.5">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="65" Y="{ly+3.8}">{value}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 底部图标展示
    lines.append('''    <ofd:TextObject ID="400" Boundary="25 220 160 5" Font="2" Size="4.5">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="60" Y="224">支持的图标类型</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 三个图标
    icon_refs = [
        ("check_icon", "成功图标"),
        ("warning_icon", "警告图标"),
        ("star_icon", "星标图标"),
    ]
    
    for i, (icon_ref, label) in enumerate(icon_refs):
        img_id = image_refs.get(icon_ref, {}).get("id", "2")
        ix = 45 + i * 50
        lines.append(f'''    <ofd:ImageObject ID="40{i+1}" Boundary="{ix} 230 16 16" ImageRef="{img_id}" CTM="1 0 0 1 0 0"/>''')
        lines.append(f'''    <ofd:TextObject ID="41{i+1}" Boundary="{ix-5} 248 26 5" Font="1" Size="3.5">
      <ofd:FillColor Value="102 102 102"/>
      <ofd:TextCode X="{ix-2}" Y="252">{label}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 页码
    lines.append('''    <ofd:TextObject ID="999" Boundary="80 285 50 5" Font="1" Size="3.8">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="85" Y="289">第 1 页 / 共 3 页</ofd:TextCode>
    </ofd:TextObject>''')

def build_mixed_content_page(lines, image_refs):
    """构建图文混排页"""
    # 页眉
    lines.append('''    <ofd:TextObject ID="500" Boundary="25 5 160 8" Font="2" Size="6.4">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="25" Y="10.5">第2页 · 图文混排示例</ofd:TextCode>
    </ofd:TextObject>''')
    
    lines.append('''    <ofd:PathObject ID="501" Boundary="25 13 160 0.3" LineWidth="0.3">
      <ofd:StrokeColor Value="0 102 204"/>
      <ofd:AbbreviatedData>M 25 13 L 185 13</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 左侧文字
    lines.append('''    <ofd:TextObject ID="502" Boundary="25 18 80 5" Font="2" Size="5">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="25" Y="22">OFD 图片嵌入能力</ofd:TextCode>
    </ofd:TextObject>''')
    
    paragraphs = [
        "OFD 版式文档支持在文档中嵌入位图图像，包括 PNG、JPEG 等常见格式。",
        "图片通过 ImageObject 对象引用，支持缩放、旋转、平移等变换操作。",
        "每张图片在公共资源区注册，通过 ImageRef 属性引用，实现高效复用。",
    ]
    
    for i, text in enumerate(paragraphs):
        py = 26 + i * 8
        lines.append(f'''    <ofd:TextObject ID="50{i+3}" Boundary="25 {py} 80 6" Font="3" Size="4">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="25" Y="{py+3.5}">{text}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 右侧图片
    img_id = image_refs.get("seal_image", {}).get("id", "5")
    lines.append(f'''    <ofd:ImageObject ID="600" Boundary="115 18 70 70" ImageRef="{img_id}" CTM="1 0 0 1 0 0"/>''')
    
    lines.append('''    <ofd:TextObject ID="601" Boundary="115 90 70 5" Font="1" Size="3.5">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="125" Y="94">图2：印章示例</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 下方图片网格
    lines.append('''    <ofd:TextObject ID="602" Boundary="25 100 160 5" Font="2" Size="4.5">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="25" Y="104">图片展示区：</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 三张小图片
    for i in range(3):
        img_id = image_refs.get(f"thumb_{i}", {}).get("id", "6")
        ix = 25 + i * 55
        lines.append(f'''    <ofd:ImageObject ID="60{i+3}" Boundary="{ix} 112 45 45" ImageRef="{img_id}" CTM="1 0 0 1 0 0"/>''')
        lines.append(f'''    <ofd:TextObject ID="61{i+3}" Boundary="{ix} 159 45 5" Font="1" Size="3.5">
      <ofd:FillColor Value="102 102 102"/>
      <ofd:TextCode X="{ix+5}" Y="163">图片 {i+1}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 技术说明
    lines.append('''    <ofd:TextObject ID="700" Boundary="25 170 160 5" Font="2" Size="4.5">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="25" Y="174">图片嵌入技术要点：</ofd:TextCode>
    </ofd:TextObject>''')
    
    tech_points = [
        "1. 图片存储在 PublicRes 或私有资源区",
        "2. 使用 MultiMedia 元素注册图片资源",
        "3. ImageObject 通过 ImageRef 引用图片",
        "4. CTM 矩阵控制图片的缩放和变换",
        "5. 支持 PNG、JPEG、BMP 等格式",
    ]
    
    for i, text in enumerate(tech_points):
        ty = 180 + i * 7
        lines.append(f'''    <ofd:TextObject ID="70{i+1}" Boundary="30 {ty} 150 5.5" Font="1" Size="4">
      <ofd:FillColor Value="85 85 85"/>
      <ofd:TextCode X="30" Y="{ty+3.8}">{text}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 页码
    lines.append('''    <ofd:TextObject ID="998" Boundary="80 285 50 5" Font="1" Size="3.8">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="85" Y="289">第 2 页 / 共 3 页</ofd:TextCode>
    </ofd:TextObject>''')

def build_icons_page(lines, image_refs):
    """构建图标展示页"""
    # 页眉
    lines.append('''    <ofd:TextObject ID="800" Boundary="25 5 160 8" Font="2" Size="6.4">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="25" Y="10.5">第3页 · 图标展示</ofd:TextCode>
    </ofd:TextObject>''')
    
    lines.append('''    <ofd:PathObject ID="801" Boundary="25 13 160 0.3" LineWidth="0.3">
      <ofd:StrokeColor Value="0 102 204"/>
      <ofd:AbbreviatedData>M 25 13 L 185 13</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 标题
    lines.append('''    <ofd:TextObject ID="802" Boundary="25 20 160 6" Font="2" Size="5">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="50" Y="25">常用图标示例</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 图标网格
    icons = [
        ("check_icon", "成功", "表示操作成功、验证通过"),
        ("warning_icon", "警告", "表示需要注意的事项"),
        ("star_icon", "星标", "表示收藏、重要标记"),
    ]
    
    for i, (icon_ref, name, desc) in enumerate(icons):
        img_id = image_refs.get(icon_ref, {}).get("id", "2")
        iy = 35 + i * 60
        
        # 图标背景
        lines.append(f'''    <ofd:PathObject ID="81{i}" Boundary="35 {iy} 140 50" LineWidth="0.3">
      <ofd:StrokeColor Value="220 220 220"/>
      <ofd:FillColor Value="250 250 252"/>
      <ofd:AbbreviatedData>M 37 {iy} L 173 {iy} L 173 {iy+50} L 37 {iy+50} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        # 图标图片
        lines.append(f'''    <ofd:ImageObject ID="82{i}" Boundary="45 {iy+10} 30 30" ImageRef="{img_id}" CTM="1 0 0 1 0 0"/>''')
        
        # 图标名称
        lines.append(f'''    <ofd:TextObject ID="83{i}" Boundary="85 {iy+12} 80 5" Font="2" Size="5">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="85" Y="{iy+16}">{name}</ofd:TextCode>
    </ofd:TextObject>''')
        
        # 图标说明
        lines.append(f'''    <ofd:TextObject ID="84{i}" Boundary="85 {iy+22} 80 10" Font="1" Size="4">
      <ofd:FillColor Value="102 102 102"/>
      <ofd:TextCode X="85" Y="{iy+26}">{desc}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 矢量图标示例（用 PathObject 绘制）
    lines.append('''    <ofd:TextObject ID="850" Boundary="25 215 160 5" Font="2" Size="4.5">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="25" Y="219">矢量图标示例（PathObject 绘制）：</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 绘制几个简单的矢量图标
    vector_icons = [
        ("home", 35, 230, "0 102 204", "首页"),
        ("user", 75, 230, "46 160 67", "用户"),
        ("settings", 115, 230, "240 150 0", "设置"),
        ("search", 155, 230, "217 30 6", "搜索"),
    ]
    
    for icon_name, ix, iy, color, label in vector_icons:
        # 图标背景圆
        lines.append(f'''    <ofd:PathObject ID="860_{icon_name}" Boundary="{ix-12} {iy-12} 24 24" LineWidth="0.5">
      <ofd:StrokeColor Value="{color}"/>
      <ofd:FillColor Value="255 255 255"/>
      <ofd:AbbreviatedData>M {ix} {iy-10} A 10 10 0 1 0 {ix+10} {iy} A 10 10 0 1 0 {ix-10} {iy} A 10 10 0 1 0 {ix} {iy-10} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        # 标签
        lines.append(f'''    <ofd:TextObject ID="870_{icon_name}" Boundary="{ix-10} {iy+15} 20 5" Font="1" Size="3.5">
      <ofd:FillColor Value="102 102 102"/>
      <ofd:TextCode X="{ix-6}" Y="{iy+19}">{label}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 页码
    lines.append('''    <ofd:TextObject ID="997" Boundary="80 285 50 5" Font="1" Size="3.8">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="85" Y="289">第 3 页 / 共 3 页</ofd:TextCode>
    </ofd:TextObject>''')

# =====================================================================
# 主函数
# =====================================================================

def main():
    """主函数：生成包含文字、图片、图标的 OFD 文件"""
    
    # 生成图片资源
    print("Generating images...")
    cover_image_data = generate_test_image(200, 160)
    seal_image_data = generate_seal_image(120)
    check_icon_data = generate_icon_image("check", 64)
    warning_icon_data = generate_icon_image("warning", 64)
    star_icon_data = generate_icon_image("star", 64)
    
    # 生成缩略图
    thumb_images = []
    for i in range(3):
        def make_pixel_func(offset):
            def pixel_func(x, y):
                r = int(255 * (x + offset) / 100 % 1)
                g = int(255 * (y + offset * 2) / 100 % 1)
                b = int(255 * (1 - x / 100))
                return (r, g, b, 255)
            return pixel_func
        thumb_images.append(create_simple_png(100, 100, make_pixel_func(i * 30)))
    
    # 定义图片引用
    image_refs = {
        "cover_image": {"id": "1", "path": "Res/cover_image.png", "data": cover_image_data},
        "check_icon": {"id": "2", "path": "Res/check_icon.png", "data": check_icon_data},
        "warning_icon": {"id": "3", "path": "Res/warning_icon.png", "data": warning_icon_data},
        "star_icon": {"id": "4", "path": "Res/star_icon.png", "data": star_icon_data},
        "seal_image": {"id": "5", "path": "Res/seal_image.png", "data": seal_image_data},
        "thumb_0": {"id": "6", "path": "Res/thumb_0.png", "data": thumb_images[0]},
        "thumb_1": {"id": "7", "path": "Res/thumb_1.png", "data": thumb_images[1]},
        "thumb_2": {"id": "8", "path": "Res/thumb_2.png", "data": thumb_images[2]},
    }
    
    # 构建 Document.xml 的图片引用列表
    doc_image_refs = {k: {"id": v["id"], "path": v["path"]} for k, v in image_refs.items()}
    
    # 内存中构建 ZIP
    print("Building OFD document...")
    buf = BytesIO()
    
    with zipfile.ZipFile(buf, 'w', zipfile.ZIP_DEFLATED) as zf:
        # OFD.xml
        zf.writestr('OFD.xml', build_ofd_xml().encode('utf-8'))
        
        # Document.xml
        zf.writestr('Doc_0/Document.xml', build_document_xml(doc_image_refs).encode('utf-8'))
        
        # 图片资源
        for ref_name, ref_info in image_refs.items():
            zf.writestr(f'Doc_0/{ref_info["path"]}', ref_info["data"])
        
        # 各页 Content.xml
        for i in range(3):
            page_xml = build_page_content(i, doc_image_refs)
            zf.writestr(f'Doc_0/Page_{i}/Content.xml', page_xml.encode('utf-8'))
    
    # 写入文件
    output_path = os.path.join(OUTPUT_DIR, '..', '图文混排示例.ofd')
    with open(output_path, 'wb') as f:
        f.write(buf.getvalue())
    
    file_size = os.path.getsize(output_path)
    print(f"\nOFD document generated successfully!")
    print(f"   File: {os.path.abspath(output_path)}")
    print(f"   Size: {file_size:,} bytes ({file_size/1024:.1f} KB)")
    print(f"   Pages: 3")
    print(f"   Content:")
    print(f"     - Page 1: 封面（文字 + 大图片 + 图标）")
    print(f"     - Page 2: 图文混排（文字环绕图片 + 图片网格）")
    print(f"     - Page 3: 图标展示（位图图标 + 矢量图标）")
    print(f"   Images: {len(image_refs)} embedded images")

if __name__ == '__main__':
    main()