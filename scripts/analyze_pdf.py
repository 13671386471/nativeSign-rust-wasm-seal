import sys
import fitz  # PyMuPDF
from pypdf import PdfReader
from pathlib import Path

pdf_path = r"D:/工作文档/1.劳动合同（2份）-3.26.pdf"

print("=== PDF 结构分析 ===")
print(f"文件: {pdf_path}")
print(f"文件大小: {Path(pdf_path).stat().st_size} bytes")

# PyMuPDF
print("\n--- PyMuPDF 分析 ---")
doc = fitz.open(pdf_path)
print(f"总页数: {doc.page_count}")
print(f"PDF 版本: {doc.metadata.get('format', 'unknown')}")
print(f"标题: {doc.metadata.get('title', '')}")
print(f"作者: {doc.metadata.get('author', '')}")
print(f"创建工具: {doc.metadata.get('creator', '')}")
print(f"Producer: {doc.metadata.get('producer', '')}")

for i in range(doc.page_count):
    page = doc[i]
    rect = page.rect
    print(f"\n第 {i+1} 页:")
    print(f"  尺寸: {rect.width} x {rect.height} pt")
    print(f"  旋转: {page.rotation}°")
    print(f"  文本字数: {len(page.get_text())}")
    print(f"  图片数量: {len(page.get_images())}")
    # 输出前200字符文本
    text = page.get_text()[:200].replace('\n', '\\n')
    print(f"  文本片段: {text}...")

# pypdf
print("\n--- pypdf 分析 ---")
reader = PdfReader(pdf_path)
print(f"总页数: {len(reader.pages)}")
print(f"PDF 版本: {reader.pdf_header}")

# 原始 PDF 文本扫描（用于理解 parse_pdf_info 的缺陷）
print("\n--- 原始文本扫描 ---")
raw = Path(pdf_path).read_bytes()
text = raw.decode('latin-1', errors='ignore')
count_matches = text.count('/Count')
print(f"/Count 出现次数: {count_matches}")
for idx in range(count_matches):
    pos = text.find('/Count', idx)
    if pos == -1:
        break
    snippet = text[pos:pos+50]
    print(f"  位置 {pos}: {snippet}")

print("\n--- /Type /Page 统计 ---")
print(f"/Type /Page 出现次数: {text.count('/Type /Page')}")
print(f"/Type/Page 出现次数: {text.count('/Type/Page')}")

doc.close()
