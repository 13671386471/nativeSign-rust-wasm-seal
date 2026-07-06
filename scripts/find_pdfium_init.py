import os

registry_dir = os.path.expanduser("~/.cargo/registry/src")
for root, dirs, files in os.walk(registry_dir):
    if "pdfium-render" in root:
        for f in files:
            if f == "mod.rs" or f == "bindings.rs":
                full_path = os.path.join(root, f)
                print(f"=== {full_path} ===")
                with open(full_path, 'r', encoding='utf-8', errors='replace') as fp:
                    print(fp.read()[:2000])
                print("---")
