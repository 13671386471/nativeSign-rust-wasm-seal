from fontTools.ttLib import TTFont

FONT = r'D:/workspace/self/rust-wasm-seal/fonts/NotoSansSC-Regular.otf'
f = TTFont(FONT)
print('sfntVersion:', f.sfntVersion)
cff = f['CFF ']
top = cff.cff.topDictIndex[0]
for attr in ['ROS', 'Charset', 'CharStrings', 'FontMatrix', 'FDSelect', 'FDArray']:
    try:
        v = getattr(top, attr)
        if attr == 'Charset':
            print('Charset len:', len(v), 'sample:', list(v)[:3])
        else:
            print(attr, '=', v)
    except Exception as e:
        print(attr, 'ERR', e)
go = f.getGlyphOrder()
print('glyph order sample:', go[:5], '... total', len(go))
cmap = f.getBestCmap()
print('cmap entries:', len(cmap))
for u in [0x4E2D, 0x6587, 0x0041, 0x0042, 0x6211]:
    print('  U+%04X -> GID %s' % (u, cmap.get(u)))
print('glyph name sample (first 3):', go[:3])
