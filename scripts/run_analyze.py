# -*- coding: utf-8 -*-
from analyze_pdf import analyze

files = [
    r"D:/工作文档/sample.pdf",
    r"D:/工作文档/阳光电源股份有限公司采购订单4500298391（电子签章专用）.pdf",
]
for f in files:
    analyze(f)
