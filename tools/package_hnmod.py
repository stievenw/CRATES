import json
import re
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "horizon-native.json"
MOD_DIR = ROOT / "mixins" / "crates"
OUTPUT = ROOT / "target" / "horizon-mod" / "crates.hnmod"
REVISION = re.compile(r"[0-9a-f]{40}\Z")


def main():
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if (
        manifest.get("schema") != 1
        or manifest.get("id") != "crates"
        or not manifest.get("version")
        or not manifest.get("game")
        or not REVISION.fullmatch(manifest.get("horizon", ""))
        or not REVISION.fullmatch(manifest.get("mixins", ""))
        or not isinstance(manifest.get("hooks"), int)
        or manifest["hooks"] < 1
    ):
        raise ValueError(f"Invalid native mod manifest: {MANIFEST}")

    mixin_manifest = MOD_DIR / "horizon-mixin.toml"
    mixin_text = mixin_manifest.read_text(encoding="utf-8")
    expected = (
        f'id = "{manifest["id"]}"',
        f'version = "{manifest["version"]}"',
        f'horizon_revision = "{manifest["horizon"]}"',
    )
    if not all(value in mixin_text for value in expected):
        raise ValueError("horizon-native.json and horizon-mixin.toml do not match")

    files = [(MANIFEST, "horizon-native.json")]
    for path in MOD_DIR.rglob("*"):
        if path.is_symlink():
            raise ValueError(f"Symlinks are not allowed in native bundles: {path}")
        if not path.is_file():
            continue
        relative = path.relative_to(ROOT).as_posix()
        if path.suffix not in {".rs", ".toml"} and "/data/" not in relative:
            raise ValueError(f"Unsupported native bundle file: {relative}")
        files.append((path, relative))
    if len(files) < 2:
        raise ValueError("No native mixin source files found")

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    partial = OUTPUT.with_suffix(".hnmod.partial")
    try:
        with zipfile.ZipFile(partial, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            for path, name in sorted(files, key=lambda item: item[1]):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                bundle.writestr(info, path.read_bytes())
        partial.replace(OUTPUT)
    finally:
        partial.unlink(missing_ok=True)

    print(f"Built {OUTPUT}")


if __name__ == "__main__":
    main()
