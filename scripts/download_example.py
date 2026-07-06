import urllib.request
url = "https://raw.githubusercontent.com/ajrcarey/pdfium-render/master/examples/index.html"
urllib.request.urlretrieve(url, "D:/workspace/front-new/dianju-ui-admin/rust-wasm-seal/examples_index.html")
print("Downloaded successfully")
