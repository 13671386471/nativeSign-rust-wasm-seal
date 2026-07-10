# 生成最小 PDF: 仅 Helvetica (标准14字体, 未嵌入) 英文文本
# 用于区分: 字体提供器整体未被 PDFium 调用  vs  仅 CID 字体(STSong-Light)不触发 MapFont
import os

content = b"BT /F1 24 Tf 72 700 Td (Hello World - Font Test) Tj ET"
objs = []
objs.append(b"<< /Type /Catalog /Pages 2 0 R >>")
objs.append(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
objs.append(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>")
objs.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
objs.append(b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream")

pdf = b"%PDF-1.4\n"
offsets = []
for i, o in enumerate(objs, start=1):
    offsets.append(len(pdf))
    pdf += str(i).encode() + b" 0 obj\n" + o + b"\nendobj\n"

xref_pos = len(pdf)
pdf += b"xref\n0 " + str(len(objs) + 1).encode() + b"\n"
pdf += b"0000000000 65535 f \n"
for off in offsets:
    pdf += ("%010d 00000 n \n" % off).encode()
pdf += b"trailer\n<< /Size " + str(len(objs) + 1).encode() + b" /Root 1 0 R >>\n"
pdf += b"startxref\n" + str(xref_pos).encode() + b"\n%%EOF\n"

out = "D:/workspace/self/rust-wasm-seal/test_helvetica.pdf"
with open(out, "wb") as f:
    f.write(pdf)
print("written", out, "size=", len(pdf))
