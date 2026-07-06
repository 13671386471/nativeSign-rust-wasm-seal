#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成 10 页 OFD 文档（符合 GB/T 33190 标准）
每页包含：文字、表格、图形（矩形/圆形/线条/折线）
"""

import hashlib
import math
import os
import zipfile
import uuid
from datetime import datetime
from io import BytesIO

OUTPUT_DIR = os.path.dirname(os.path.abspath(__file__))
NS = "http://www.ofdspec.org/2016"

def ofd_tag(tag):
    """生成带 OFD 命名空间的标签"""
    return f'<?xml version="1.0" encoding="UTF-8"?>'

def xml_decl():
    return '<?xml version="1.0" encoding="UTF-8"?>'

def build_ofd_xml():
    """构建 OFD.xml — 文档入口"""
    doc_id = uuid.uuid4().hex
    return f'''{xml_decl()}
<ofd:OFD xmlns:ofd="{NS}" DocType="OFD" Version="1.0">
  <ofd:DocBody>
    <ofd:DocInfo>
      <ofd:DocID>{doc_id}</ofd:DocID>
      <ofd:Title>示例OFD文档 - 含表格图形文字</ofd:Title>
      <ofd:Author>WorkBuddy AI</ofd:Author>
      <ofd:Subject>OFD示例文档</ofd:Subject>
      <ofd:Abstract>一个包含10页的OFD文档，每页含有文字、表格和图形元素</ofd:Abstract>
      <ofd:CreationDate>{datetime.now().strftime('%Y-%m-%d')}</ofd:CreationDate>
      <ofd:Creator>WorkBuddy OFD Generator</ofd:Creator>
    </ofd:DocInfo>
    <ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>
  </ofd:DocBody>
</ofd:OFD>'''

def build_document_xml():
    """构建 Document.xml — 文档结构，注册所有页面"""
    pages_xml = []
    for i in range(10):
        pages_xml.append(f'      <ofd:Page ID="{i+1}" BaseLoc="Page_{i}/Content.xml"/>')
    
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
        <ofd:Font ID="4" FontName="仿宋" FamilyName="FangSong" Charset="GB2312"/>
      </ofd:Fonts>
    </ofd:PublicRes>
  </ofd:CommonData>
  <ofd:Pages>
{chr(10).join(pages_xml)}
  </ofd:Pages>
  <ofd:Annotations/>
</ofd:Document>'''

# =====================================================================
# 每页内容生成器
# =====================================================================

def page_header(title, page_num, content_lines):
    """生成页面头部 — 标题 + 分隔线"""
    content_lines.append(f'''    <ofd:TextObject ID="{1000+page_num}" Boundary="25 5 160 8" Font="2" Size="6.4">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="25" Y="10.5">{title}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 标题下分隔线
    content_lines.append(f'''    <ofd:PathObject ID="{1100+page_num}" Boundary="25 13 160 0.3" LineWidth="0.3">
      <ofd:StrokeColor Value="0 102 204"/>
      <ofd:AbbreviatedData>M 25 13 L 185 13</ofd:AbbreviatedData>
    </ofd:PathObject>''')

def add_text_paragraph(content_lines, obj_id, x, y, text, font_id=3, size=4.5, color="51 51 51"):
    """添加一段文字"""
    content_lines.append(f'''    <ofd:TextObject ID="{obj_id}" Boundary="{x} {y} 160 {size+2}" Font="{font_id}" Size="{size}">
      <ofd:FillColor Value="{color}"/>
      <ofd:TextCode X="{x}" Y="{y+size}">{text}</ofd:TextCode>
    </ofd:TextObject>''')

def add_table(content_lines, tbl_id, x, y, headers, rows, col_widths=None):
    """添加一个表格
    headers: 表头列表
    rows: 数据行列表（每行也是列表）
    col_widths: 列宽列表（单位mm），不传则均分总宽100mm
    """
    num_cols = len(headers)
    total_w = 160  # 表格总宽 mm
    if col_widths:
        cw = col_widths
    else:
        cw = [total_w / num_cols] * num_cols
    
    row_h = 7  # 行高 mm
    header_h = 8
    
    # 累积 X 坐标
    cx = []
    acc = x
    for w in cw:
        cx.append(acc)
        acc += w
    
    # 表头背景
    content_lines.append(f'''    <ofd:PathObject ID="{tbl_id}_hbg" Boundary="{x} {y} {total_w} {header_h}" LineWidth="0">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:AbbreviatedData>M {x} {y} L {acc} {y} L {acc} {y+header_h} L {x} {y+header_h} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 表头文字
    for j, h in enumerate(headers):
        content_lines.append(f'''    <ofd:TextObject ID="{tbl_id}_h{j}" Boundary="{cx[j]} {y} {cw[j]} {header_h}" Font="2" Size="4">
      <ofd:FillColor Value="255 255 255"/>
      <ofd:TextCode X="{cx[j]+1}" Y="{y+5.5}">{h}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 数据行
    for i, row in enumerate(rows):
        ry = y + header_h + i * row_h
        
        # 行背景（交替色）
        bg = "245 248 252" if i % 2 == 0 else "255 255 255"
        content_lines.append(f'''    <ofd:PathObject ID="{tbl_id}_r{i}_bg" Boundary="{x} {ry} {total_w} {row_h}" LineWidth="0">
      <ofd:FillColor Value="{bg}"/>
      <ofd:AbbreviatedData>M {x} {ry} L {acc} {ry} L {acc} {ry+row_h} L {x} {ry+row_h} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        for j, cell in enumerate(row):
            content_lines.append(f'''    <ofd:TextObject ID="{tbl_id}_r{i}c{j}" Boundary="{cx[j]} {ry} {cw[j]} {row_h}" Font="3" Size="3.8">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="{cx[j]+1}" Y="{ry+4.8}">{str(cell)}</ofd:TextCode>
    </ofd:TextObject>''')
    
    # 表格边框
    table_bottom = y + header_h + len(rows) * row_h
    content_lines.append(f'''    <ofd:PathObject ID="{tbl_id}_border" Boundary="{x-0.3} {y-0.3} {total_w+0.6} {table_bottom-y+0.6}" LineWidth="0.3">
      <ofd:StrokeColor Value="180 180 180"/>
      <ofd:AbbreviatedData>M {x} {y} L {acc} {y} L {acc} {table_bottom} L {x} {table_bottom} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    return table_bottom  # 返回表格底部 Y 坐标

def add_graphics_demo(content_lines, gid, x, y, w=160, h=70):
    """添加图形演示区域：矩形、圆形、线条、折线、三角形"""
    shapes = []
    
    # 圆角矩形边框
    shapes.append(f'''    <ofd:PathObject ID="{gid}_rect1" Boundary="{x} {y} {w} {h}" LineWidth="0.3">
      <ofd:StrokeColor Value="200 200 200"/>
      <ofd:FillColor Value="250 250 252"/>
      <ofd:AbbreviatedData>M {x+2} {y} L {x+w-2} {y} Q {x+w} {y} {x+w} {y+2} L {x+w} {y+h-2} Q {x+w} {y+h} {x+w-2} {y+h} L {x+2} {y+h} Q {x} {y+h} {x} {y+h-2} L {x} {y+2} Q {x} {y} {x+2} {y} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 圆形
    cx1, cy1, r1 = x + 30, y + 25, 15
    shapes.append(f'''    <ofd:PathObject ID="{gid}_circle" Boundary="{cx1-r1-1} {cy1-r1-1} {(r1+1)*2} {(r1+1)*2}" LineWidth="0.8">
      <ofd:StrokeColor Value="217 30 6"/>
      <ofd:FillColor Value="255 230 228"/>
      <ofd:AbbreviatedData>M {cx1} {cy1-r1} A {r1} {r1} 0 1 0 {cx1+r1} {cy1} A {r1} {r1} 0 1 0 {cx1-r1} {cy1} A {r1} {r1} 0 1 0 {cx1} {cy1-r1} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 矩形
    rx, ry, rw, rh = x + 60, y + 10, 30, 30
    shapes.append(f'''    <ofd:PathObject ID="{gid}_rect2" Boundary="{rx-0.5} {ry-0.5} {rw+1} {rh+1}" LineWidth="0.8">
      <ofd:StrokeColor Value="2 125 50"/>
      <ofd:FillColor Value="228 255 237"/>
      <ofd:AbbreviatedData>M {rx} {ry} L {rx+rw} {ry} L {rx+rw} {ry+rh} L {rx} {ry+rh} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 三角形
    tx, ty = x + 115, y + 40
    shapes.append(f'''    <ofd:PathObject ID="{gid}_triangle" Boundary="{tx-16} {ty-16} 32 32" LineWidth="0.8">
      <ofd:StrokeColor Value="234 150 0"/>
      <ofd:FillColor Value="255 243 220"/>
      <ofd:AbbreviatedData>M {tx} {ty-15} L {tx+13} {ty+13} L {tx-13} {ty+13} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 菱形
    dx, dy = x + 150, y + 25
    shapes.append(f'''    <ofd:PathObject ID="{gid}_diamond" Boundary="{dx-12} {dy-12} 24 24" LineWidth="0.8">
      <ofd:StrokeColor Value="120 30 160"/>
      <ofd:FillColor Value="240 225 250"/>
      <ofd:AbbreviatedData>M {dx} {dy-10} L {dx+10} {dy} L {dx} {dy+10} L {dx-10} {dy} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 水平线/折线/箭头
    shapes.append(f'''    <ofd:PathObject ID="{gid}_line1" Boundary="{x+135} {y+8} 25 12" LineWidth="0.5">
      <ofd:StrokeColor Value="100 100 100"/>
      <ofd:AbbreviatedData>M {x+140} {y+14} L {x+155} {y+14}</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    shapes.append(f'''    <ofd:PathObject ID="{gid}_line2" Boundary="{x+135} {y+8} 25 20" LineWidth="0.5">
      <ofd:StrokeColor Value="100 100 100"/>
      <ofd:AbbreviatedData>M {x+140} {y+14} L {x+140} {y+20} L {x+155} {y+20} L {x+155} {y+26}</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    # 标签
    shapes.append(f'''    <ofd:TextObject ID="{gid}_lbl1" Boundary="{cx1-r1} {cy1+r1+2} {40} 5" Font="3" Size="3.5">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="{cx1-8}" Y="{cy1+r1+5}">圆形</ofd:TextCode>
    </ofd:TextObject>''')
    
    shapes.append(f'''    <ofd:TextObject ID="{gid}_lbl2" Boundary="{rx} {ry+rh+2} {rw} 5" Font="3" Size="3.5">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="{rx+3}" Y="{ry+rh+5}">矩形</ofd:TextCode>
    </ofd:TextObject>''')
    
    shapes.append(f'''    <ofd:TextObject ID="{gid}_lbl3" Boundary="{tx-10} {ty+16} 20 5" Font="3" Size="3.5">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="{tx-6}" Y="{ty+19}">三角形</ofd:TextCode>
    </ofd:TextObject>''')
    
    content_lines.extend(shapes)

def page_footer(content_lines, page_num):
    """页码和底部装饰线"""
    content_lines.append(f'''    <ofd:PathObject ID="{3100+page_num}" Boundary="25 283 160 0.2" LineWidth="0.2">
      <ofd:StrokeColor Value="200 200 200"/>
      <ofd:AbbreviatedData>M 25 283 L 185 283</ofd:AbbreviatedData>
    </ofd:PathObject>''')
    
    content_lines.append(f'''    <ofd:TextObject ID="{3200+page_num}" Boundary="80 284 50 5" Font="3" Size="3.8">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="85" Y="288">第 {page_num+1} 页 / 共 10 页</ofd:TextCode>
    </ofd:TextObject>''')

# =====================================================================
# 各页内容
# =====================================================================

def build_page_content(page_num):
    """构建某一页的 Content.xml"""
    lines = []
    current_id = 0
    
    # --- 第 1 页: 封面 ---
    if page_num == 0:
        page_header("OFD 示例文档 - 表格、图形、文字综合展示", page_num, lines)
        
        # 大标题
        lines.append(f'''    <ofd:TextObject ID="2001" Boundary="25 35 160 10" Font="2" Size="8">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="50" Y="42">OFD 版式文档综合示例</ofd:TextCode>
    </ofd:TextObject>''')
        
        # 副标题
        lines.append(f'''    <ofd:TextObject ID="2002" Boundary="25 48 160 6" Font="4" Size="4.5">
      <ofd:FillColor Value="102 102 102"/>
      <ofd:TextCode X="58" Y="52">—— 涵盖文字排版、数据表格、几何图形 ——</ofd:TextCode>
    </ofd:TextObject>''')
        
        # 分隔装饰
        lines.append(f'''    <ofd:PathObject ID="2003" Boundary="60 58 90 1" LineWidth="0.8">
      <ofd:StrokeColor Value="0 102 204"/>
      <ofd:AbbreviatedData>M 60 58.4 L 150 58.4</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        # 文档信息说明
        info_lines = [
            ("文档格式:", "OFD (GB/T 33190-2016)", "中国国家标准版式文档格式"),
            ("页数:", "10 页", "封面 + 8页内容 + 封底"),
            ("生成工具:", "WorkBuddy OFD Generator", "基于 Python 自动化生成"),
            ("内容类型:", "文字 / 表格 / 图形 / 图表", "综合示例"),
            ("字体:", "宋体 / 黑体 / 楷体 / 仿宋", "四种中文字体"),
        ]
        
        for i, (label, value, desc) in enumerate(info_lines):
            ly = 65 + i * 9
            lines.append(f'''    <ofd:TextObject ID="201{i+4}" Boundary="30 {ly} 30 5" Font="2" Size="4.5">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="30" Y="{ly+3.8}">{label}</ofd:TextCode>
    </ofd:TextObject>''')
            lines.append(f'''    <ofd:TextObject ID="202{i+4}" Boundary="60 {ly} 80 5" Font="3" Size="4.5">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="60" Y="{ly+3.8}">{value}</ofd:TextCode>
    </ofd:TextObject>''')
            lines.append(f'''    <ofd:TextObject ID="203{i+4}" Boundary="130 {ly} 55 5" Font="1" Size="4">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="130" Y="{ly+3.8}">{desc}</ofd:TextCode>
    </ofd:TextObject>''')
        
        # 装饰图形
        add_graphics_demo(lines, 3000, 25, 117, 160, 60)
        
        # 底部说明
        lines.append(f'''    <ofd:TextObject ID="2099" Boundary="25 185 160 5" Font="1" Size="3.8">
      <ofd:FillColor Value="153 153 153"/>
      <ofd:TextCode X="60" Y="189">本文档由 WorkBuddy AI 自动生成，符合 GB/T 33190 OFD 标准</ofd:TextCode>
    </ofd:TextObject>''')
    
    # --- 第 2 页: 文字排版示例 ---
    elif page_num == 1:
        page_header("第2页 · 文字排版示例", page_num, lines)
        
        paragraphs = [
            ("汉字是世界上使用人数最多的文字之一。汉字是迄今为止连续使用时间最长的文字，也是上古时期各大文字体系中唯一传承至今的文字，中国历代皆以汉字为主要官方文字。", 20),
            ("中国古代称汉字为\u201c字\u201d或\u201c文字\u201d。秦始皇统一中国后，推行\u201c书同文\u201d政策，以小篆作为标准字体。汉代隶书成为官方文书的主要字体，楷书在魏晋时期逐渐定型。印刷术发明后，宋体字成为最常用的印刷字体。", 34),
            ("进入信息时代，计算机文字处理技术飞速发展。中文信息处理从早期的点阵字库发展到矢量字库，再到现代的 OpenType 可变字体技术。Unicode 编码体系的建立，使得汉字能够在全球各种计算机平台上无障碍流通和使用。", 43),
            ("版式文档（Fixed-layout Document）是指版面固定的文档格式。与流式文档（如 HTML、Word）不同，版式文档的呈现效果在不同设备上保持一致，不会因为设备差异而导致排版变化。OFD（Open Fixed-layout Document）是中国自主研发的版式文档格式标准。", 52),
            ("OFD 格式具有以下特点：一是自主可控，采用 XML 描述文档结构，开放透明；二是体积小巧，支持多种压缩算法；三是安全性高，支持数字签名和电子签章；四是扩展性强，可方便地添加自定义扩展。", 61),
            ("本文档使用 OFD 格式生成，旨在展示 OFD 文档对各种内容元素的支持能力。后续页面将分别展示表格、图形、组合内容等多种排版形式。", 70),
        ]
        
        for i, (text, y) in enumerate(paragraphs):
            add_text_paragraph(lines, 2100 + i, 25, y, text)
        
        # 字体展示区
        lines.append(f'''    <ofd:TextObject ID="2198" Boundary="25 78 80 5" Font="2" Size="4.5">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="25" Y="82">字体预览：</ofd:TextCode>
    </ofd:TextObject>''')
        
        font_samples = [
            ("宋体 · 这是宋体字示例 SimSun", 1, 86),
            ("黑体 · 这是黑体字示例 SimHei", 2, 93),
            ("楷体 · 这是楷体字示例 KaiTi", 3, 100),
            ("仿宋 · 这是仿宋字示例 FangSong", 4, 107),
        ]
        for text, fid, fy in font_samples:
            lines.append(f'''    <ofd:TextObject ID="2200" Boundary="25 {fy} 160 5.5" Font="{fid}" Size="4.8">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="30" Y="{fy+4}">{text}</ofd:TextCode>
    </ofd:TextObject>''')
        
        page_footer(lines, page_num)
    
    # --- 第 3 页: 数据表格 ---
    elif page_num == 2:
        page_header("第3页 · 数据表格示例", page_num, lines)
        
        add_text_paragraph(lines, 3100, 25, 16, "OFD 支持在文档中嵌入结构化的表格数据。以下展示几种常见的表格类型：")
        
        # 表格1: 简单的员工信息表
        headers1 = ["姓名", "部门", "职位", "入职日期"]
        rows1 = [
            ["张三", "技术部", "高级工程师", "2020-03-15"],
            ["李四", "产品部", "产品经理", "2021-06-01"],
            ["王五", "设计部", "UI设计师", "2019-09-20"],
            ["赵六", "市场部", "市场总监", "2018-01-10"],
            ["孙七", "技术部", "前端开发", "2022-07-05"],
        ]
        bottom1 = add_table(lines, 3201, 25, 22, headers1, rows1, [32, 32, 40, 56])
        
        # 表格2: 项目进度表
        add_text_paragraph(lines, 3200, 25, bottom1 + 5, "项目进度跟踪表：", font_id=2, size=4.5)
        headers2 = ["项目名称", "负责人", "进度", "状态"]
        rows2 = [
            ["OA系统升级", "张三", "85%", "进行中"],
            ["移动端开发", "李四", "60%", "进行中"],
            ["数据中台", "王五", "100%", "已完成"],
            ["报表系统", "赵六", "30%", "延期"],
        ]
        add_table(lines, 3202, 25, bottom1 + 12, headers2, rows2, [50, 35, 35, 40])
        
        page_footer(lines, page_num)
    
    # --- 第 4 页: 财务数据表格 ---
    elif page_num == 3:
        page_header("第4页 · 财务报表示例", page_num, lines)
        
        add_text_paragraph(lines, 4100, 25, 16, "以下为2025年度营收数据汇总表：", font_id=2)
        
        headers3 = ["季度", "营收(万元)", "成本(万元)", "利润(万元)", "利润率"]
        rows3 = [
            ["Q1", "1,250.8", "876.3", "374.5", "29.9%"],
            ["Q2", "1,468.2", "952.1", "516.1", "35.2%"],
            ["Q3", "1,632.5", "1,045.8", "586.7", "35.9%"],
            ["Q4", "1,890.0", "1,198.3", "691.7", "36.6%"],
            ["合计", "6,241.5", "4,072.5", "2,169.0", "34.8%"],
        ]
        add_table(lines, 4201, 25, 24, headers3, rows3, [30, 35, 35, 35, 25])
        
        add_text_paragraph(lines, 4202, 25, 78, "月度营收趋势（折线示意）：", font_id=2, size=4.5)
        
        # 柱状图模拟（用 PathObject 绘制）
        bar_data = [980, 1150, 1320, 1450, 1600, 1780]
        bar_labels = ["1月", "2月", "3月", "4月", "5月", "6月"]
        bar_colors = ["0 102 204", "46 160 67", "240 150 0", "217 30 6", "120 30 160", "0 150 180"]
        chart_x = 35
        chart_y = 84
        chart_h = 55
        bar_w = 18
        gap = 5
        max_val = 2000
        
        # 坐标轴
        lines.append(f'''    <ofd:PathObject ID="4300_axis" Boundary="{chart_x-2} {chart_y-2} 160 {chart_h+10}" LineWidth="0.3">
      <ofd:StrokeColor Value="180 180 180"/>
      <ofd:AbbreviatedData>M {chart_x} {chart_y} L {chart_x} {chart_y+chart_h} L {chart_x+150} {chart_y+chart_h}</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        for i, val in enumerate(bar_data):
            bh = val / max_val * chart_h
            bx = chart_x + 5 + i * (bar_w + gap)
            by = chart_y + chart_h - bh
            lines.append(f'''    <ofd:PathObject ID="4301_{i}" Boundary="{bx-0.5} {by-0.5} {bar_w+1} {bh+1}" LineWidth="0">
      <ofd:FillColor Value="{bar_colors[i]}"/>
      <ofd:AbbreviatedData>M {bx} {by} L {bx+bar_w} {by} L {bx+bar_w} {chart_y+chart_h} L {bx} {chart_y+chart_h} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
            
            # 数值标签
            lines.append(f'''    <ofd:TextObject ID="4302_{i}" Boundary="{bx-3} {by-6} {bar_w+6} 4.5" Font="1" Size="3.5">
      <ofd:FillColor Value="102 102 102"/>
      <ofd:TextCode X="{bx}" Y="{by-1.5}">{val}万</ofd:TextCode>
    </ofd:TextObject>''')
            
            # 横轴标签
            lines.append(f'''    <ofd:TextObject ID="4303_{i}" Boundary="{bx} {chart_y+chart_h+2} {bar_w+6} 4" Font="1" Size="3.5">
      <ofd:FillColor Value="102 102 102"/>
      <ofd:TextCode X="{bx+2}" Y="{chart_y+chart_h+4.8}">{bar_labels[i]}</ofd:TextCode>
    </ofd:TextObject>''')
        
        add_text_paragraph(lines, 4399, 25, chart_y + chart_h + 9, "图：2025年上半年月度营收柱状图", font_id=1, size=3.5, color="153 153 153")
        
        page_footer(lines, page_num)
    
    # --- 第 5 页: 图形示例 ---
    elif page_num == 4:
        page_header("第5页 · 几何图形示例", page_num, lines)
        
        add_text_paragraph(lines, 5100, 25, 16, "OFD 使用 SVG 风格路径数据（AbbreviatedData）描述矢量图形，支持 M/L/Q/C/A/Z 等绘图指令。")
        
        # 第一组图形
        add_graphics_demo(lines, 5201, 25, 24, 160, 60)
        
        # 流程图示例
        add_text_paragraph(lines, 5200, 25, 92, "简单流程图示例：", font_id=2, size=4.5)
        
        # 绘制流程图
        flow_y = 100
        boxes = [("开始", 80, flow_y, 26, 8, "46 160 67"), 
                ("数据输入", 80, flow_y+14, 26, 8, "0 102 204"),
                ("数据处理", 80, flow_y+28, 26, 8, "240 150 0"),
                ("结果输出", 80, flow_y+42, 26, 8, "217 30 6"),
                ("结束", 80, flow_y+56, 26, 8, "120 30 160")]
        
        for text, bx, by, bw, bh, color in boxes:
            rx = 4  # 圆角
            # 圆角矩形
            lines.append(f'''    <ofd:PathObject ID="5310" Boundary="{bx-0.5} {by-0.5} {bw+1} {bh+1}" LineWidth="0.5">
      <ofd:StrokeColor Value="{color}"/>
      <ofd:FillColor Value="255 255 255"/>
      <ofd:AbbreviatedData>M {bx+rx} {by} L {bx+bw-rx} {by} Q {bx+bw} {by} {bx+bw} {by+rx} L {bx+bw} {by+bh-rx} Q {bx+bw} {by+bh} {bx+bw-rx} {by+bh} L {bx+rx} {by+bh} Q {bx} {by+bh} {bx} {by+bh-rx} L {bx} {by+rx} Q {bx} {by} {bx+rx} {by} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
            lines.append(f'''    <ofd:TextObject ID="5311" Boundary="{bx} {by} {bw} {bh}" Font="2" Size="4">
      <ofd:FillColor Value="{color}"/>
      <ofd:TextCode X="{bx+5}" Y="{by+5.5}">{text}</ofd:TextCode>
    </ofd:TextObject>''')
        
        # 箭头（流程连线）
        arrow_y_positions = [flow_y+8, flow_y+22, flow_y+36, flow_y+50]
        for ay in arrow_y_positions:
            lines.append(f'''    <ofd:PathObject ID="5320" Boundary="88 {ay} 10 6" LineWidth="0.4">
      <ofd:StrokeColor Value="150 150 150"/>
      <ofd:AbbreviatedData>M 93 {ay} L 93 {ay+4} L 96 {ay+2} M 93 {ay+4} L 90 {ay+2}</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        page_footer(lines, page_num)
    
    # --- 第 6 页: 混合排版 ---
    elif page_num == 5:
        page_header("第6页 · 混合排版示例（文字+表格+图形）", page_num, lines)
        
        add_text_paragraph(lines, 6100, 25, 16, "本章展示如何在单页中混合使用文字、表格和图形元素，实现丰富的排版效果。")
        
        # 左侧文字
        add_text_paragraph(lines, 6101, 25, 25, "OFD 版式文档支持丰富的排版能力。", font_id=2, size=4.8, color="0 102 204")
        add_text_paragraph(lines, 6102, 25, 32, "在同一个页面中，可以灵活地放置文本段落、数据表格、矢量图形等元素。OFD 使用 XML 描述页面内容，图层（Layer）机制使得内容管理更加清晰。每个页面包含一个或多个图层，图层中包含各种图形对象。", font_id=1, size=4)
        
        # 小表格
        headers4 = ["序号", "功能", "说明"]
        rows4 = [
            ["1", "TextObject", "文本对象 - 单行/多行文字"],
            ["2", "PathObject", "路径对象 - 矢量图形"],
            ["3", "ImageObject", "图像对象 - 位图/图片"],
            ["4", "CompositeObject", "复合对象 - 组合图形"],
        ]
        add_table(lines, 6201, 25, 45, headers4, rows4, [22, 50, 88])
        
        # 右侧图形
        add_text_paragraph(lines, 6300, 25, 85, "OFD 对象类型占比（饼图示意）：", font_id=2, size=4.5)
        
        # 饼图
        pcx, pcy, pr = 55, 106, 25
        
        # 简化的饼图（用路径近似）
        # 实际上对于精确饼图，可以计算扇形路径
        lines.append(f'''    <ofd:PathObject ID="6310_bg" Boundary="{pcx-pr-2} {pcy-pr-2} {pr*2+4} {pr*2+4}" LineWidth="0.3">
      <ofd:StrokeColor Value="200 200 200"/>
      <ofd:FillColor Value="255 255 255"/>
      <ofd:AbbreviatedData>M {pcx} {pcy-pr} A {pr} {pr} 0 1 0 {pcx+pr} {pcy} A {pr} {pr} 0 1 0 {pcx-pr} {pcy} A {pr} {pr} 0 1 0 {pcx} {pcy-pr} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        # 图例
        legend_items = [
            ("TextObject  45%", "0 102 204"),
            ("PathObject  30%", "240 150 0"),
            ("ImageObject 15%", "217 30 6"),
            ("其他        10%", "120 30 160"),
        ]
        for i, (label, color) in enumerate(legend_items):
            ly = 98 + i * 7
            lines.append(f'''    <ofd:PathObject ID="6320_{i}" Boundary="{pcx+pr+5} {ly} 5 5" LineWidth="0">
      <ofd:FillColor Value="{color}"/>
      <ofd:AbbreviatedData>M {pcx+pr+5} {ly} L {pcx+pr+10} {ly} L {pcx+pr+10} {ly+5} L {pcx+pr+5} {ly+5} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
            lines.append(f'''    <ofd:TextObject ID="6330_{i}" Boundary="{pcx+pr+13} {ly} 50 5" Font="1" Size="3.8">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="{pcx+pr+13}" Y="{ly+4}">{label}</ofd:TextCode>
    </ofd:TextObject>''')
        
        page_footer(lines, page_num)
    
    # --- 第 7 页: 技术架构 ---
    elif page_num == 6:
        page_header("第7页 · 技术参数与规格", page_num, lines)
        
        add_text_paragraph(lines, 7100, 25, 16, "OFD 文档格式技术参数一览：", font_id=2, size=5)
        
        headers5 = ["参数类别", "参数项", "参数值"]
        rows5 = [
            ["文档结构", "标准编号", "GB/T 33190-2016"],
            ["文档结构", "文件扩展名", ".ofd"],
            ["文档结构", "压缩格式", "ZIP (Deflate)"],
            ["内容描述", "描述语言", "XML 1.0"],
            ["内容描述", "命名空间", "http://www.ofdspec.org/2016"],
            ["页面", "最大页数", "无限制"],
            ["页面", "默认尺寸", "A4 (210×297mm)"],
            ["图形", "矢量描述", "SVG 路径数据"],
            ["图形", "颜色空间", "RGB / CMYK / Gray"],
            ["字体", "嵌入方式", "内嵌 / 引用"],
            ["安全", "数字签名", "支持 GM/T 0031"],
            ["安全", "电子签章", "支持 GM/T 0031 标准"],
        ]
        add_table(lines, 7201, 25, 24, headers5, rows5, [38, 50, 72])
        
        # 技术特点
        add_text_paragraph(lines, 7300, 25, 125, "OFD核心技术特点：", font_id=2, size=4.8, color="0 102 204")
        
        features = [
            ("自主可控", "由中国主导制定，知识产权自主可控，不受国外技术制约"),
            ("开放透明", "基于XML和ZIP开放格式，任何厂商均可实现读写功能"),
            ("高度安全", "内置数字签名和电子签章机制，确保文档不可篡改"),
            ("体积精巧", "支持多种压缩算法，尤善处理扫描件大体积文档"),
            ("广泛适用", "覆盖电子证照、电子发票、电子合同、电子病历等场景"),
        ]
        for i, (title, desc) in enumerate(features):
            fy = 132 + i * 10
            lines.append(f'''    <ofd:TextObject ID="731{i}" Boundary="30 {fy} 20 6" Font="2" Size="4.5">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="30" Y="{fy+4.5}">{title}</ofd:TextCode>
    </ofd:TextObject>''')
            lines.append(f'''    <ofd:TextObject ID="732{i}" Boundary="55 {fy} 130 6" Font="3" Size="4">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="55" Y="{fy+4.5}">{desc}</ofd:TextCode>
    </ofd:TextObject>''')
        
        page_footer(lines, page_num)
    
    # --- 第 8 页: 复杂表格 ---
    elif page_num == 7:
        page_header("第8页 · 复杂数据表格", page_num, lines)
        
        add_text_paragraph(lines, 8100, 25, 16, "以下为产品对比分析表：", font_id=2, size=5)
        
        headers6 = ["产品名称", "版本", "价格(元)", "用户数", "评价", "推荐指数"]
        rows6 = [
            ["TrustedSign 企业版", "v3.2", "¥98,000", "500+", "优秀", "★★★★★"],
            ["TrustedSign 标准版", "v3.2", "¥28,000", "100", "良好", "★★★★"],
            ["电子签章平台A", "v2.1", "¥58,000", "300", "中等", "★★★"],
            ["电子签章平台B", "v1.8", "¥35,000", "150", "一般", "★★★"],
            ["开源OFD工具C", "v0.9", "免费", "不限", "入门", "★★"],
            ["桌面签章软件D", "v5.0", "¥8,000", "单机", "良好", "★★★★"],
        ]
        add_table(lines, 8201, 25, 24, headers6, rows6, [35, 20, 28, 22, 25, 30])
        
        add_text_paragraph(lines, 8300, 25, 78, "各产品特征对比（雷达示意）：", font_id=2, size=4.5)
        
        # 绘制一个简化的雷达图框架
        rx, ry, rr = 80, 110, 30
        pentagon_points = []
        for i in range(5):
            angle = -math.pi / 2 + 2 * math.pi * i / 5
            px = rx + rr * math.cos(angle)
            py = ry + rr * math.sin(angle)
            pentagon_points.append((px, py))
        
        # 五边形框架
        if pentagon_points:
            pts_str = " ".join([f"L {px:.1f} {py:.1f}" for px, py in pentagon_points])
            lines.append(f'''    <ofd:PathObject ID="8310" Boundary="{rx-rr-2} {ry-rr-2} {rr*2+4} {rr*2+4}" LineWidth="0.3">
      <ofd:StrokeColor Value="180 180 180"/>
      <ofd:FillColor Value="252 252 255"/>
      <ofd:AbbreviatedData>M {pentagon_points[0][0]:.1f} {pentagon_points[0][1]:.1f} {pts_str} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
            
            # 中心到各顶点的连线
            for px, py in pentagon_points:
                lines.append(f'''    <ofd:PathObject ID="8311" Boundary="{rx-rr} {ry-rr} {rr*2} {rr*2}" LineWidth="0.15">
      <ofd:StrokeColor Value="220 220 220"/>
      <ofd:AbbreviatedData>M {rx:.1f} {ry:.1f} L {px:.1f} {py:.1f}</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        labels_radar = ["安全性", "易用性", "性价比", "扩展性", "兼容性"]
        for i, label in enumerate(labels_radar):
            angle = -3.14159/2 + 2 * 3.14159 * i / 5
            label_angle = -math.pi / 2 + 2 * math.pi * i / 5
            lx = rx + (rr + 10) * math.cos(label_angle)
            ly = ry + (rr + 10) * math.sin(label_angle)
            lines.append(f'''    <ofd:TextObject ID="832{i}" Boundary="{lx-15} {ly-3} 30 6" Font="2" Size="3.8">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="{lx-8}" Y="{ly+3}">{label}</ofd:TextCode>
    </ofd:TextObject>''')
        
        page_footer(lines, page_num)
    
    # --- 第 9 页: 流程图 ---
    elif page_num == 8:
        page_header("第9页 · 业务流程示例", page_num, lines)
        
        add_text_paragraph(lines, 9100, 25, 16, "电子合同签署业务流程：", font_id=2, size=5, color="0 102 204")
        
        # 横向流程图
        steps = [
            ("发起签署", 25, 25, 28, 10, "0 102 204"),
            ("身份认证", 60, 25, 28, 10, "46 160 67"),
            ("选择印章", 95, 25, 28, 10, "240 150 0"),
            ("意愿确认", 130, 25, 28, 10, "217 30 6"),
            ("生成签章", 165, 25, 28, 10, "120 30 160"),
        ]
        
        for text, sx, sy, sw, sh, color in steps:
            # 圆角矩形步骤
            lines.append(f'''    <ofd:PathObject ID="9200" Boundary="{sx-0.3} {sy-0.3} {sw+0.6} {sh+0.6}" LineWidth="0.5">
      <ofd:StrokeColor Value="{color}"/>
      <ofd:FillColor Value="255 255 255"/>
      <ofd:AbbreviatedData>M {sx+4} {sy} L {sx+sw-4} {sy} Q {sx+sw} {sy} {sx+sw} {sy+4} L {sx+sw} {sy+sh-4} Q {sx+sw} {sy+sh} {sx+sw-4} {sy+sh} L {sx+4} {sy+sh} Q {sx} {sy+sh} {sx} {sy+sh-4} L {sx} {sy+4} Q {sx} {sy} {sx+4} {sy} Z</ofd:AbbreviatedData>
    </ofd:PathObject>''')
            lines.append(f'''    <ofd:TextObject ID="9201" Boundary="{sx} {sy+2} {sw} {sh-4}" Font="2" Size="4">
      <ofd:FillColor Value="{color}"/>
      <ofd:TextCode X="{sx+3}" Y="{sy+7.2}">{text}</ofd:TextCode>
    </ofd:TextObject>''')
        
        # 箭头
        for ax in [53, 88, 123, 158]:
            lines.append(f'''    <ofd:PathObject ID="9210" Boundary="{ax} 28 6 4" LineWidth="0.5">
      <ofd:StrokeColor Value="180 180 180"/>
      <ofd:AbbreviatedData>M {ax} 30 L {ax+3} 30 M {ax+2} 29 L {ax+4} 30 L {ax+2} 31</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        # 详细说明
        add_text_paragraph(lines, 9300, 25, 42, "各步骤详细说明：", font_id=2, size=4.8, color="51 51 51")
        
        step_details = [
            ("1. 发起签署：", "合同起草完成后，发起人指定签署方并设置签署顺序，系统生成签署任务并通知相关参与方。支持顺序签署、无序签署和并行签署三种模式。"),
            ("2. 身份认证：", "签署人通过短信验证码、人脸识别、UKey证书、银行打款等多种方式进行身份认证，确保签署主体真实有效。认证通过后签署人获得签署权限。"),
            ("3. 选择印章：", "签署人在文档中定位签章位置，从可用印章列表中选择合适的印章。支持公章、合同章、财务章、法人章等多种印章类型。"),
            ("4. 意愿确认：", "签署人再次确认签署内容和签署位置，通过二次认证（如短信验证码）确认签署意愿。此步骤是电子签章法律效力的关键环节。"),
            ("5. 生成签章：", "系统使用签署人数字证书对文档进行数字签名，将签名值、证书信息和印章图像合成到文档中，生成具有法律效力的电子签章。"),
        ]
        
        for i, (title, desc) in enumerate(step_details):
            dy = 50 + i * 13.5
            lines.append(f'''    <ofd:TextObject ID="931{i}" Boundary="25 {dy} 25 5.5" Font="2" Size="4">
      <ofd:FillColor Value="51 51 51"/>
      <ofd:TextCode X="25" Y="{dy+4}">{title}</ofd:TextCode>
    </ofd:TextObject>''')
            lines.append(f'''    <ofd:TextObject ID="932{i}" Boundary="52 {dy} 133 10" Font="1" Size="3.8">
      <ofd:FillColor Value="85 85 85"/>
      <ofd:TextCode X="52" Y="{dy+4}">{desc}</ofd:TextCode>
    </ofd:TextObject>''')
        
        add_text_paragraph(lines, 9400, 25, 125, "图：电子合同签署完整业务流程", font_id=1, size=3.5, color="153 153 153")
        
        page_footer(lines, page_num)
    
    # --- 第 10 页: 封底 ---
    elif page_num == 9:
        page_header("第10页 · 封底", page_num, lines)
        
        # 中央大标题
        lines.append(f'''    <ofd:TextObject ID="A001" Boundary="25 60 160 10" Font="2" Size="8">
      <ofd:FillColor Value="0 102 204"/>
      <ofd:TextCode X="55" Y="67">感谢阅览</ofd:TextCode>
    </ofd:TextObject>''')
        
        # 装饰线
        lines.append(f'''    <ofd:PathObject ID="A002" Boundary="55 75 100 0.5" LineWidth="0.5">
      <ofd:StrokeColor Value="0 102 204"/>
      <ofd:AbbreviatedData>M 55 75.2 L 155 75.2</ofd:AbbreviatedData>
    </ofd:PathObject>''')
        
        # 说明文字
        closing_text = [
            "本文档为 OFD（Open Fixed-layout Document）格式的示例文档。",
            "OFD 是中国自主研发的新一代版式文档格式标准（GB/T 33190），",
            "具有自主可控、开放透明、安全可靠等特点。",
            "",
            "文档内容涵盖：",
            "· 文字排版（多种字体、段落）",
            "· 数据表格（基本表格、复杂对比表）",
            "· 矢量图形（圆形、矩形、三角形、菱形、折线）",
            "· 业务图表（柱状图、饼图、流程图、雷达图）",
            "",
            "本文档由 WorkBuddy AI 自动生成",
            f"生成日期：{datetime.now().strftime('%Y年%m月%d日')}",
            "生成工具：Python + ZIP + XML",
        ]
        
        for i, text in enumerate(closing_text):
            ty = 82 + i * 6.5
            if text:
                lines.append(f'''    <ofd:TextObject ID="A10{i}" Boundary="40 {ty} 130 5.5" Font="3" Size="4.2">
      <ofd:FillColor Value="85 85 85"/>
      <ofd:TextCode X="40" Y="{ty+3.8}">{text}</ofd:TextCode>
    </ofd:TextObject>''')
        
        # 装饰图形
        add_graphics_demo(lines, 5000, 25, 160, 160, 55)
        
        page_footer(lines, page_num)
    
    # 组装 Page Content XML
    content_str = '\n'.join(lines)
    return f'''{xml_decl()}
<ofd:Page xmlns:ofd="{NS}">
  <ofd:Content>
    <ofd:Layer ID="1" Type="Body" DrawParam="1">
{content_str}
    </ofd:Layer>
  </ofd:Content>
</ofd:Page>'''


def main():
    """主函数：生成 OFD 文件"""
    
    # 内存中构建 ZIP
    buf = BytesIO()
    
    with zipfile.ZipFile(buf, 'w', zipfile.ZIP_DEFLATED) as zf:
        # OFD.xml
        zf.writestr('OFD.xml', build_ofd_xml().encode('utf-8'))
        
        # Document.xml
        zf.writestr('Doc_0/Document.xml', build_document_xml().encode('utf-8'))
        
        # 各页 Content.xml
        for i in range(10):
            page_xml = build_page_content(i)
            zf.writestr(f'Doc_0/Page_{i}/Content.xml', page_xml.encode('utf-8'))
    
    # 写入文件
    output_path = os.path.join(OUTPUT_DIR, '..', '示例OFD文档_10页演示版.ofd')
    with open(output_path, 'wb') as f:
        f.write(buf.getvalue())
    
    file_size = os.path.getsize(output_path)
    print(f"OFD document generated successfully!")
    print(f"   File: {os.path.abspath(output_path)}")
    print(f"   Size: {file_size:,} bytes ({file_size/1024:.1f} KB)")
    print(f"   Pages: 10")
    print(f"   Content: Text / Tables / Graphics / Charts")


if __name__ == '__main__':
    main()
