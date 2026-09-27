"""Prepare identical fresh tracked corpus snapshots; never copies prior graphs."""
import io
import pathlib
import subprocess
import sys
import zipfile

source, commit, root, name = sys.argv[1:]
if not name or pathlib.Path(name).name != name:
    raise ValueError("Corpus name must be a single path segment")
data = subprocess.check_output(["git", "-C", source, "archive", "--format=zip", commit])
for tool in ("astria", "graphify"):
    dest = pathlib.Path(root) / (name + "-" + tool)
    dest.mkdir()
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        for entry in archive.infolist():
            parts = pathlib.PurePosixPath(entry.filename).parts
            if any(p in {".astria", ".graphify", "graphify-out", "bench-work", "node_modules", "target"} for p in parts):
                continue
            if entry.filename.startswith("/") or ".." in parts:
                raise ValueError("Unsafe archive path")
            archive.extract(entry, dest)
