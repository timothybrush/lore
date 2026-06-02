# SPDX-FileCopyrightText: 2026 Epic Games, Inc.
# SPDX-License-Identifier: MIT
import logging
import os
import random
import subprocess
import time
from pathlib import Path

import pytest

from lore import Lore

logger = logging.getLogger(__name__)


@pytest.mark.slow
def test_store_compaction(new_lore_repo, lore_executable_path):
    repo: Lore = new_lore_repo()

    # Generate 10k files
    for i in range(10):
        subpath = str(i)
        for j in range(10):
            subsubpath = os.path.join(subpath, str(j))
            repo.make_dirs(subsubpath)
            for k in range(1000):
                with repo.open_file(
                    os.path.join(subsubpath, str(k) + ".uasset"), "w+b"
                ) as output_file:
                    output_file.write(os.urandom(10 + k + (i * k * j)))

    # Also one big file for re-fragmentation
    repo.make_dirs(os.path.join("large", "file"))
    with repo.open_file(os.path.join("large", "file", "test.png"), "w+b") as output_file:
        output_file.write(os.urandom(160 * 1024 * 1024))

    # Add a copy for deduplication
    repo.copy_file(
        os.path.join("large", "file", "test.png"),
        os.path.join("large", "file", "test2.png"),
    )

    # Commit local to ensure data gets written to local store
    repo.stage(scan=True)
    repo.commit("Generate files", local=True)
    repo.push(max_connections=16)

    # Set the repo max size to 100MiB
    path = Path(os.path.join(repo.dot_path(), "config.toml"))
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)

    for i, line in enumerate(lines):
        if line.strip().startswith("max_size"):
            lines[i] = "max_size = 500_000_000\n"

    path.write_text("".join(lines), encoding="utf-8")

    # Run some commands to potentially trigger the gc
    repo.status(gc=True)
    repo.history(gc=True)
    repo.repository_verify()

    for i in range(0, 100):
        if i % 2 == 0:
            p = subprocess.Popen(
                [
                    lore_executable_path,
                    "--repository",
                    repo.path,
                    "--gc",
                    "--debug",
                    "repository",
                    "verify",
                ]
            )

            time.sleep(random.uniform(1.0, 5.0))

            p.terminate()
        else:
            repo.repository_verify(debug=True, gc=True)

    repo.repository_verify()

    # Verify full gc run
    repo.repository_gc(debug=True)
    repo.repository_verify()
