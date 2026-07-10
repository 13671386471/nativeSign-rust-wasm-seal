import sys
from pypdf import PdfReader
from pypdf.generic import IndirectObject, DictionaryObject, ArrayObject

def resolve(o):
    if isinstance(o, IndirectObject):
        return o.get_object()
    return o

def walk_resources(res, out, depth=0):
    res = resolve(res)
    if not isinstance(res, DictionaryObject):
        return
    # Fonts
    fonts = resolve(res.get("/Font"))
    if fonts:
        for k, v in resolve(fonts).items():
            f = resolve(v)
            subtype = f.get("/Subtype")
            base = f.get("/BaseFont")
            # embedded?
            embedded = False
            fd = f.get("/FontDescriptor")
            if fd:
                fd = resolve(fd)
                for ef in ("/FontFile", "/FontFile2", "/FontFile3"):
                    if fd.get(ef):
                        embedded = True
            out.append(f"  Font {k}: Subtype={subtype} BaseFont={base} Embedded={embedded}")
    # XObjects
    xobjs = resolve(res.get("/XObject"))
    if xobjs:
        for k, v in resolve(xobjs).items():
            x = resolve(v)
            st = x.get("/Subtype")
            if st == "/Image":
                filt = x.get("/Filter")
                cs = x.get("/ColorSpace")
                w = x.get("/Width"); h = x.get("/Height")
                out.append(f"  XObject {k}: Image Filter={filt} ColorSpace={cs} {w}x{h}")
            elif st == "/Form":
                out.append(f"  XObject {k}: Form (nested)")
                ws = resolve(x.get("/Resources"))
                if ws:
                    walk_resources(ws, out, depth+1)
    # ExtGState (transparency / blend modes)
    gs = resolve(res.get("/ExtGState"))
    if gs:
        for k, v in resolve(gs).items():
            g = resolve(v)
            bm = g.get("/BM"); ca = g.get("/ca"); CA = g.get("/CA")
            if bm or ca is not None or CA is not None:
                out.append(f"  ExtGState {k}: BM={bm} ca={ca} CA={CA}")
    # OCG
    oc = resolve(res.get("/Properties"))
    if oc:
        for k, v in resolve(oc).items():
            o = resolve(v)
            if o.get("/Type") == "/OCG":
                out.append(f"  OCG {k}: {o.get('/Name')}")

def analyze(path):
    print("="*70)
    print("FILE:", path)
    print("="*70)
    try:
        r = PdfReader(path)
    except Exception as e:
        print("  ERROR reading:", e)
        return
    print("Encrypted:", r.is_encrypted)
    print("Page count:", len(r.pages))
    for i, page in enumerate(r.pages):
        p = page.get("/Page") if False else page
        mb = p.mediabox
        rot = p.get("/Rotate")
        out = []
        out.append(f"--- Page {i} ---")
        out.append(f"  MediaBox: {mb}  Rotate: {rot}")
        # transparency group?
        grp = p.get("/Group")
        if grp:
            grp = resolve(grp)
            out.append(f"  Page Group: /S={grp.get('/S')} /CS={grp.get('/CS')} /K={grp.get('/K')}")
        res = p.get("/Resources")
        if res:
            walk_resources(res, out)
        else:
            out.append("  (no /Resources)")
        # text presence
        try:
            txt = page.extract_text() or ""
            out.append(f"  Extracted text length: {len(txt)}  preview: {txt[:60]!r}")
        except Exception as e:
            out.append(f"  text extract error: {e}")
        print("\n".join(out))
    print()

if __name__ == "__main__":
    for p in sys.argv[1:]:
        analyze(p)
